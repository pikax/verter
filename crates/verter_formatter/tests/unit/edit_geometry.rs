use crate::coordinates::{AuthoredFormatByteOffset as O, AuthoredFormatByteRange as R};
use crate::doc::{Doc, Line};
use crate::edit::{derive_edits, derive_edits_from_doc, EditError, FormatEdit, FormatEditSet};
use crate::provenance::{AuthoredProvenance, FormatProvenanceTable, Provenanced};
use crate::render::{render, RenderOptions, RenderedDoc, RenderedSegment, Segment};

/// Derive edits for one table row: `anchors` are `(a_start, a_end, r_start,
/// r_end)` claimed retained units in rendered order; every rendered span
/// between them is generated content.
fn derive_case(
    authored: &str,
    rendered: &str,
    anchors: &[(u32, u32, u32, u32)],
) -> Result<FormatEditSet, EditError> {
    let mut table = FormatProvenanceTable::new(0_u32);
    let mut segments = Vec::new();
    let mut cursor = 0_u32;
    for &(a0, a1, r0, r1) in anchors {
        assert!(r0 >= cursor, "test anchors must be in rendered order");
        if r0 > cursor {
            segments.push(Segment::Generated(RenderedSegment {
                range: crate::coordinates::FormattedByteRange::for_emitted(rendered, cursor, r0)
                    .unwrap(),
            }));
        }
        // Claims are minted inversion-checked only: validating them against
        // the authored source is the derivation's job, including refusals.
        let range = R::new(O::from_raw(a0), O::from_raw(a1)).unwrap();
        let id = table.bind_authored(range);
        segments.push(Segment::Source(Provenanced::new(
            RenderedSegment {
                range: crate::coordinates::FormattedByteRange::for_emitted(rendered, r0, r1)
                    .unwrap(),
            },
            AuthoredProvenance::new(id, range),
        )));
        cursor = r1;
    }
    if cursor as usize != rendered.len() {
        segments.push(Segment::Generated(RenderedSegment {
            range: crate::coordinates::FormattedByteRange::for_emitted(
                rendered,
                cursor,
                rendered.len() as u32,
            )
            .unwrap(),
        }));
    }
    derive_edits(authored, rendered, &segments)
}

fn tuples(set: &FormatEditSet) -> Vec<(u32, u32, String)> {
    set.edits
        .iter()
        .map(|edit| {
            (
                edit.range.start().get(),
                edit.range.end().get(),
                edit.text.clone(),
            )
        })
        .collect()
}

/// The acceptance invariants every derived set must satisfy: sorted by
/// authored position, non-overlapping, in bounds at scalar boundaries, and
/// free of no-op edits.
fn assert_well_formed(set: &FormatEditSet, authored: &str) {
    for pair in set.edits.windows(2) {
        assert!(
            pair[1].range.start().get() >= pair[0].range.end().get(),
            "edits overlap or are unsorted: {:?}",
            tuples(set)
        );
    }
    for edit in &set.edits {
        R::for_source(authored, edit.range.start(), edit.range.end()).unwrap();
        assert!(
            !(edit.range.is_empty() && edit.text.is_empty()),
            "no-op edit: {edit:?}"
        );
        if !edit.range.is_empty() {
            let start = edit.range.start().get() as usize;
            let end = edit.range.end().get() as usize;
            assert_ne!(
                edit.text,
                authored[start..end],
                "edit replaces bytes with identical bytes"
            );
        }
    }
}

fn row(
    name: &str,
    authored: &str,
    rendered: &str,
    anchors: &[(u32, u32, u32, u32)],
    expected: &[(u32, u32, &str)],
) {
    let set = derive_case(authored, rendered, anchors).unwrap();
    assert_well_formed(&set, authored);
    let actual = tuples(&set);
    let expected: Vec<_> = expected
        .iter()
        .map(|&(start, end, text)| (start, end, text.to_owned()))
        .collect();
    assert_eq!(actual, expected, "{name}: unexpected edit set");
    // Deterministic: the same inputs derive the same set.
    assert_eq!(derive_case(authored, rendered, anchors).unwrap(), set);
    assert_eq!(
        set.apply(authored).unwrap(),
        rendered,
        "{name}: applied edits must reproduce the rendered bytes exactly"
    );
}

#[test]
fn locked_edit_law_examples_emit_exactly_the_locked_edits() {
    row(
        "two interior insertions with stable middle",
        "a =1;\r\nb =2;\r\n",
        "a = 1;\r\nb = 2;\r\n",
        &[(0, 3, 0, 3), (3, 10, 4, 11), (10, 14, 12, 16)],
        &[(3, 3, " "), (10, 10, " ")],
    );
    row(
        "unicode insertion between retained scalars",
        "é🙂",
        "é 🙂",
        &[(0, 2, 0, 2), (2, 6, 3, 7)],
        &[(2, 2, " ")],
    );
}

#[test]
fn table_cases_apply_exactly_for_every_edit_kind() {
    row(
        "identical bytes with full tiling",
        "ab",
        "ab",
        &[(0, 1, 0, 1), (1, 2, 1, 2)],
        &[],
    );
    row("identical bytes without anchors", "ab", "ab", &[], &[]);
    row("empty authored and rendered", "", "", &[], &[]);
    row(
        "insert whole text into empty source",
        "",
        "new",
        &[],
        &[(0, 0, "new")],
    );
    row(
        "pure deletion in the middle",
        "a  b",
        "a b",
        &[(0, 1, 0, 1), (3, 4, 2, 3)],
        &[(2, 3, "")],
    );
    row(
        "pure replacement",
        "one two",
        "one;two",
        &[(0, 3, 0, 3), (4, 7, 4, 7)],
        &[(3, 4, ";")],
    );
    row(
        "prefix replacement without anchors",
        "ab",
        "Xb",
        &[],
        &[(0, 1, "X")],
    );
    row(
        "suffix replacement without anchors",
        "ab",
        "aY",
        &[],
        &[(1, 2, "Y")],
    );
    row(
        "whole-gap replacement without anchors",
        "ab",
        "XY",
        &[],
        &[(0, 2, "XY")],
    );
    row(
        "deletion before one anchor",
        "ab",
        "b",
        &[(1, 2, 0, 1)],
        &[(0, 1, "")],
    );
    row(
        "insertion after one anchor",
        "ab",
        "abX",
        &[(0, 1, 0, 1)],
        &[(2, 2, "X")],
    );
    row(
        "eof final newline insertion",
        "x",
        "x\n",
        &[(0, 1, 0, 1)],
        &[(1, 1, "\n")],
    );
    row(
        "bom deletion keeps retained tail",
        "\u{feff}x",
        "x",
        &[(3, 4, 0, 1)],
        &[(0, 3, "")],
    );
    row(
        "crlf to lf deletes each carriage return",
        "a\r\nb\r\n",
        "a\nb\n",
        &[(0, 1, 0, 1), (3, 4, 2, 3)],
        &[(1, 2, ""), (4, 5, "")],
    );
    row(
        "lf to crlf inserts each carriage return",
        "a\nb\n",
        "a\r\nb\r\n",
        &[(0, 1, 0, 1), (2, 3, 3, 4)],
        &[(1, 1, "\r"), (3, 3, "\r")],
    );
    row(
        "multibyte replacement at scalar boundaries",
        "é🙂",
        "🙂é",
        &[],
        &[(0, 6, "🙂é")],
    );
    row(
        "equal generated gap content emits nothing",
        "ab",
        "ab",
        &[(0, 1, 0, 1)],
        &[],
    );
}

#[test]
fn unequal_retained_claims_do_not_fabricate_anchors() {
    // The claimed binding's bytes differ, so the unit is gap content, not a
    // stable anchor; derivation still applies exactly.
    row(
        "transformed claim becomes one replacement",
        "abc",
        "aXc",
        &[(0, 2, 0, 2)],
        &[(1, 2, "X")],
    );
    // Equal bytes claimed out of authored order are a move, not a bijection:
    // demote to gap content and express it as delete plus insert.
    row(
        "moved retained unit is delete plus insert",
        "ab",
        "ba",
        &[(1, 2, 0, 1), (0, 1, 1, 2)],
        &[(0, 1, ""), (2, 2, "a")],
    );
}

#[test]
fn adjacent_gaps_coalesce_only_when_no_retained_byte_separates_them() {
    // A zero-width unit in both spaces separates nothing: the two generated
    // runs coalesce into one insertion.
    row(
        "zero-width unit does not split one insertion",
        "b",
        "XYb",
        &[(0, 0, 1, 1), (0, 1, 2, 3)],
        &[(0, 0, "XY")],
    );
    // A real retained byte separates the same runs into two edits.
    row(
        "retained byte separates adjacent insertions",
        "ab",
        "XaYb",
        &[(0, 1, 1, 2), (1, 2, 3, 4)],
        &[(0, 0, "X"), (1, 1, "Y")],
    );
}

#[test]
fn stable_prefix_suffix_and_internal_regions_stay_excluded() {
    // Equal head and tail are stripped even with no anchors at all: one
    // single-byte edit, never a whole-file replacement.
    let head = "H".repeat(1024);
    let authored = format!("{head}mT");
    let rendered = format!("{head}NT");
    let set = derive_case(&authored, &rendered, &[]).unwrap();
    assert_well_formed(&set, &authored);
    assert_eq!(tuples(&set), vec![(1024, 1025, "N".to_owned())]);
    assert_eq!(set.apply(&authored).unwrap(), rendered);

    // The locked example already proves the internal stable region: two
    // insertions, not one whole-file replacement.
    let set = derive_case(
        "a =1;\r\nb =2;\r\n",
        "a = 1;\r\nb = 2;\r\n",
        &[(0, 3, 0, 3), (3, 10, 4, 11), (10, 14, 12, 16)],
    )
    .unwrap();
    assert_ne!(
        set.edits,
        vec![FormatEdit {
            range: R::for_source("a =1;\r\nb =2;\r\n", O::from_raw(0), O::from_raw(14)).unwrap(),
            text: "a = 1;\r\nb = 2;\r\n".to_owned(),
        }],
        "a smaller edit set is proven: whole-file replacement is rejected"
    );

    // Anchored head and tail with one changed middle: only the middle edits.
    let set = derive_case(
        "headMIDtail",
        "headNEWtail",
        &[(0, 4, 0, 4), (7, 11, 7, 11)],
    )
    .unwrap();
    assert_well_formed(&set, "headMIDtail");
    assert_eq!(tuples(&set), vec![(4, 7, "NEW".to_owned())]);
}

#[test]
fn pipeline_derivation_from_a_rendered_doc_applies_exactly() {
    let authored = "let x=1;\nlet  y=2;\n";
    let mut table = FormatProvenanceTable::new(41_u32);
    let bind = |table: &mut FormatProvenanceTable<u32>, start: u32, end: u32| {
        let range = R::for_source(authored, O::from_raw(start), O::from_raw(end)).unwrap();
        AuthoredProvenance::new(table.bind_authored(range), range)
    };
    let mut doc = Doc::new();
    let mut nodes = Vec::new();
    for (start, end) in [(0_u32, 5_u32), (6, 9), (9, 12), (14, 16), (16, 19)] {
        nodes.push(doc.source(Provenanced::new(
            authored[start as usize..end as usize].to_owned(),
            bind(&mut table, start, end),
        )));
        if let Some((gap_start, gap_end)) = match (start, end) {
            (0, 5) => Some((5_usize, 6_usize)),
            (6, 9) => None,
            (9, 12) => Some((12, 14)),
            (14, 16) => None,
            _ => None,
        } {
            nodes.push(doc.text(&authored[gap_start..gap_end]));
        }
        if let Some(text) = match (start, end) {
            (0, 5) => Some(" = "),
            (6, 9) => Some(""),
            (9, 12) => Some(" "),
            (14, 16) => Some(" "),
            _ => None,
        } {
            nodes.push(doc.text(text));
        }
    }
    let hard = doc.line(Line::Hard);
    let root = doc.concat(nodes);
    let tail = doc.concat([root, hard]);
    let rendered: RenderedDoc = render(&doc, tail, RenderOptions::default()).unwrap();
    let set = derive_edits_from_doc(authored, &rendered).unwrap();
    assert_well_formed(&set, authored);
    assert_eq!(
        set.apply(authored).unwrap(),
        rendered.text,
        "applied edits must reproduce the rendered document exactly"
    );
    // Retained units are anchors: no edit may intersect their authored ranges.
    for anchor in table.iter() {
        for edit in &set.edits {
            let (a0, a1) = (anchor.range().start().get(), anchor.range().end().get());
            let (e0, e1) = (edit.range.start().get(), edit.range.end().get());
            assert!(
                e1 <= a0 || e0 >= a1,
                "edit {e0}..{e1} intersects retained unit {a0}..{a1}"
            );
        }
    }
    // A document that reproduces its source byte for byte derives zero edits.
    let mut doc = Doc::new();
    let mut nodes = Vec::new();
    for (start, end) in [(0_u32, 9_u32), (9, 19)] {
        nodes.push(doc.source(Provenanced::new(
            authored[start as usize..end as usize].to_owned(),
            bind(&mut table, start, end),
        )));
    }
    let root = doc.concat(nodes);
    let same = render(&doc, root, RenderOptions::default()).unwrap();
    assert_eq!(same.text, authored);
    assert!(derive_edits_from_doc(authored, &same)
        .unwrap()
        .edits
        .is_empty());
}

fn adversary_bound(set: &FormatEditSet, authored: &str, rendered: &str) -> usize {
    let bound = 4 * (authored.len() + rendered.len()) + 64;
    assert!(
        set.work.edit_comparisons <= bound,
        "edit_comparisons {} exceeds the locked ceiling {bound}",
        set.work.edit_comparisons
    );
    let emitted: usize = set.edits.iter().map(|edit| edit.text.len()).sum();
    assert!(
        emitted <= rendered.len(),
        "edit text bytes {emitted} exceed the rendered size"
    );
    set.work.edit_comparisons
}

/// Alternating bytes with nothing in common: prefix and suffix stripping each
/// stop at the first scalar, so comparison work is constant in the size.
fn adversary_alternating(size: usize) -> usize {
    let authored: String = "ab".repeat(size);
    let rendered: String = "ba".repeat(size);
    let set = derive_case(&authored, &rendered, &[]).unwrap();
    assert_well_formed(&set, &authored);
    assert_eq!(set.apply(&authored).unwrap(), rendered);
    adversary_bound(&set, &authored, &rendered)
}

/// One changed byte behind a long equal prefix and before an equal suffix.
fn adversary_long_prefix(size: usize) -> usize {
    let authored = format!("{}mT", "a".repeat(size));
    let rendered = format!("{}NT", "a".repeat(size));
    let set = derive_case(&authored, &rendered, &[]).unwrap();
    assert_eq!(
        tuples(&set),
        vec![(size as u32, size as u32 + 1, "N".to_owned())]
    );
    adversary_bound(&set, &authored, &rendered)
}

/// Anchor-dense layout: every retained byte alternates with a generated byte,
/// maximizing gaps, anchors and replacements per byte.
fn adversary_anchor_dense(size: usize) -> usize {
    let authored = "a".repeat(size);
    let rendered: String = (0..size)
        .map(|i| if i % 2 == 0 { 'a' } else { 'X' })
        .collect();
    let anchors: Vec<_> = (0..size)
        .step_by(2)
        .map(|i| (i as u32, i as u32 + 1, i as u32, i as u32 + 1))
        .collect();
    let set = derive_case(&authored, &rendered, &anchors).unwrap();
    assert_well_formed(&set, &authored);
    assert_eq!(set.apply(&authored).unwrap(), rendered);
    adversary_bound(&set, &authored, &rendered)
}

#[test]
fn adversarial_pairs_stay_within_the_locked_work_bounds() {
    let (a1, a2) = (adversary_alternating(1000), adversary_alternating(2000));
    assert!(a2 <= a1 + 4, "no common structure must stay constant work");
    let (p1, p2) = (adversary_long_prefix(1000), adversary_long_prefix(2000));
    assert!(p2 <= 2 * p1 + 64, "prefix stripping must stay linear");
    let (d1, d2) = (adversary_anchor_dense(1000), adversary_anchor_dense(2000));
    assert!(d2 <= 2 * d1 + 64, "anchor verification must stay linear");
}

mod nightly_scale {
    #[test]
    fn adversarial_pairs_at_scale() {
        assert!(super::adversary_alternating(1_000_000) <= 8);
        super::adversary_long_prefix(1_000_000);
        super::adversary_anchor_dense(1_000_000);
    }
}

#[test]
fn malformed_rendered_segments_and_retained_claims_are_refused() {
    // Segments must tile the rendered text exactly.
    let authored = "ab";
    let rendered = "ab";
    let mut table = FormatProvenanceTable::new(0_u32);
    let range = R::for_source(authored, O::from_raw(1), O::from_raw(2)).unwrap();
    let id = table.bind_authored(range);
    let segments = vec![
        Segment::Generated(RenderedSegment {
            range: crate::coordinates::FormattedByteRange::for_emitted(rendered, 0, 1).unwrap(),
        }),
        Segment::Source(Provenanced::new(
            RenderedSegment {
                range: crate::coordinates::FormattedByteRange::for_emitted(rendered, 2, 2).unwrap(),
            },
            AuthoredProvenance::new(id, range),
        )),
    ];
    assert_eq!(
        derive_edits(authored, rendered, &segments),
        Err(EditError::SegmentsNotContiguous { index: 1 })
    );
    // A claimed range past the end of the authored source is refused.
    let set = derive_case("ab", "ab", &[(0, 5, 0, 2)]).unwrap_err();
    assert_eq!(
        set,
        EditError::InvalidRetainedRange {
            index: 0,
            error: crate::coordinates::CoordinateError::OutOfBounds {
                offset: 5,
                source_len: 2
            }
        }
    );
    // A claimed range inside a multibyte scalar is refused.
    let set = derive_case("é🙂", "ab", &[(0, 1, 0, 1)]).unwrap_err();
    assert_eq!(
        set,
        EditError::InvalidRetainedRange {
            index: 0,
            error: crate::coordinates::CoordinateError::NotCharBoundary { offset: 1 }
        }
    );
}

fn hand_edit(start: u32, end: u32, text: &str) -> FormatEdit {
    // Inversion-checked only: validating the range against the source is
    // `apply`'s job, including the refusals under test.
    FormatEdit {
        range: R::new(O::from_raw(start), O::from_raw(end)).unwrap(),
        text: text.to_owned(),
    }
}

#[test]
fn applying_rejects_malformed_edit_sets() {
    let authored = "abcdef";
    let malformed = |edits: Vec<FormatEdit>, expected: EditError| {
        let set = FormatEditSet {
            edits,
            work: Default::default(),
        };
        assert_eq!(set.apply(authored), Err(expected));
    };
    malformed(
        vec![hand_edit(5, 6, "x"), hand_edit(1, 2, "y")],
        EditError::EditsOutOfOrder { index: 1 },
    );
    malformed(
        vec![hand_edit(0, 2, "x"), hand_edit(1, 3, "y")],
        EditError::EditsOutOfOrder { index: 1 },
    );
    malformed(
        vec![hand_edit(9, 10, "x")],
        EditError::InvalidEdit {
            index: 0,
            error: crate::coordinates::CoordinateError::OutOfBounds {
                offset: 10,
                source_len: 6,
            },
        },
    );
    malformed(vec![hand_edit(3, 3, "")], EditError::NoOpEdit { index: 0 });
    // A well-formed set still applies: sorted insertions at one boundary
    // follow stored (rendered) order.
    let set = FormatEditSet {
        edits: vec![hand_edit(3, 3, "X"), hand_edit(3, 3, "Y")],
        work: Default::default(),
    };
    assert_eq!(set.apply(authored).unwrap(), "abcXYdef");
}
