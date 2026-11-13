//! Minimal non-overlapping edit geometry: exact application, order and
//! non-overlap, minimal boundaries and bounded derivation work.

use verter_formatter::coordinates::{
    AuthoredFormatByteOffset, AuthoredFormatByteRange, CoordinateError, FormattedByteOffset,
    FormattedByteRange,
};
use verter_formatter::doc::{Doc, Line};
use verter_formatter::edit::{
    derive_edits, derive_edits_from_doc, EditError, FormatEdit, FormatEditSet,
};
use verter_formatter::provenance::{AuthoredProvenance, FormatProvenanceTable, Provenanced};
use verter_formatter::render::{render, LineEnding, RenderOptions, RenderedSegment, Segment};

/// Mint an authored range against the exact text it indexes.
fn authored_range(text: &str, start: usize, end: usize) -> AuthoredFormatByteRange {
    AuthoredFormatByteRange::for_source(
        text,
        AuthoredFormatByteOffset::for_source(text, start).unwrap(),
        AuthoredFormatByteOffset::for_source(text, end).unwrap(),
    )
    .unwrap()
}

/// Mint a rendered range against the exact text it indexes.
fn rendered_range(text: &str, start: usize, end: usize) -> FormattedByteRange {
    FormattedByteRange::for_source(
        text,
        FormattedByteOffset::for_source(text, start).unwrap(),
        FormattedByteOffset::for_source(text, end).unwrap(),
    )
    .unwrap()
}

fn generated(rendered: &str, start: usize, end: usize) -> Segment {
    Segment::Generated(RenderedSegment {
        range: rendered_range(rendered, start, end),
    })
}

fn source(
    table: &mut FormatProvenanceTable<u32>,
    rendered: &str,
    (start, end): (usize, usize),
    claim: AuthoredFormatByteRange,
) -> Segment {
    Segment::Source(Provenanced::new(
        RenderedSegment {
            range: rendered_range(rendered, start, end),
        },
        AuthoredProvenance::new(table.bind_authored(claim), claim),
    ))
}

/// Tile `rendered` from `anchors`, each `(a_start, a_end, r_start, r_end)`
/// a source-backed segment in rendered order whose binding is minted against
/// `claims` (normally the authored text itself); every rendered span between
/// them is generated content.
fn tile(claims: &str, rendered: &str, anchors: &[(usize, usize, usize, usize)]) -> Vec<Segment> {
    let mut table = FormatProvenanceTable::new(0_u32);
    let mut segments = Vec::new();
    let mut cursor = 0;
    for &(a0, a1, r0, r1) in anchors {
        assert!(r0 >= cursor, "anchors must be in rendered order");
        if r0 > cursor {
            segments.push(generated(rendered, cursor, r0));
        }
        segments.push(source(
            &mut table,
            rendered,
            (r0, r1),
            authored_range(claims, a0, a1),
        ));
        cursor = r1;
    }
    if cursor != rendered.len() {
        segments.push(generated(rendered, cursor, rendered.len()));
    }
    segments
}

fn derive_case(
    authored: &str,
    rendered: &str,
    anchors: &[(usize, usize, usize, usize)],
) -> Result<FormatEditSet, EditError> {
    derive_edits(authored, rendered, &tile(authored, rendered, anchors))
}

fn tuples(set: &FormatEditSet) -> Vec<(u32, u32, String)> {
    set.edits()
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

fn splits_crlf_pair(text: &str, offset: u32) -> bool {
    let offset = offset as usize;
    offset > 0
        && offset < text.len()
        && text.as_bytes()[offset - 1] == b'\r'
        && text.as_bytes()[offset] == b'\n'
}

/// Edits are sorted by authored position, non-overlapping, addressable at
/// scalar boundaries, free of no-op edits, never bounded inside a CRLF pair.
fn assert_well_formed(set: &FormatEditSet, authored: &str) {
    for pair in set.edits().windows(2) {
        assert!(
            pair[1].range.start().get() >= pair[0].range.end().get(),
            "edits overlap or are unsorted: {:?}",
            tuples(set)
        );
    }
    for edit in set.edits() {
        AuthoredFormatByteRange::for_source(authored, edit.range.start(), edit.range.end())
            .unwrap();
        let (start, end) = (
            edit.range.start().get() as usize,
            edit.range.end().get() as usize,
        );
        assert_ne!(edit.text, authored[start..end], "no-op edit: {edit:?}");
        assert!(
            !splits_crlf_pair(authored, edit.range.start().get()),
            "edit start splits a CRLF pair: {edit:?}"
        );
        assert!(
            !splits_crlf_pair(authored, edit.range.end().get()),
            "edit end splits a CRLF pair: {edit:?}"
        );
    }
    // The set rebuilds through the checked constructor and applies the same.
    let rebuilt = FormatEditSet::new(authored, set.edits().to_vec()).unwrap();
    assert_eq!(rebuilt.edits(), set.edits());
}

/// One table row: the derived set is exactly `expected`, well formed,
/// deterministic, and applies to the authored bytes to the rendered bytes.
fn row(
    name: &str,
    authored: &str,
    rendered: &str,
    anchors: &[(usize, usize, usize, usize)],
    expected: &[(u32, u32, &str)],
) {
    let set = derive_case(authored, rendered, anchors).unwrap();
    assert_well_formed(&set, authored);
    let expected: Vec<_> = expected
        .iter()
        .map(|&(start, end, text)| (start, end, text.to_owned()))
        .collect();
    assert_eq!(tuples(&set), expected, "{name}: unexpected edit set");
    assert_eq!(
        derive_case(authored, rendered, anchors).unwrap(),
        set,
        "{name}: derivation is deterministic"
    );
    assert_eq!(
        set.apply(authored).unwrap(),
        rendered,
        "{name}: applied edits must reproduce the rendered bytes exactly"
    );
}

/// The two locked `edits.examples` of `fixtures/formatter/laws.json`.
#[test]
fn locked_edit_law_examples_emit_exactly_the_locked_edits() {
    row(
        "two interior insertions with a stable middle",
        "a =1;\r\nb =2;\r\n",
        "a = 1;\r\nb = 2;\r\n",
        &[(0, 3, 0, 3), (3, 10, 4, 11), (10, 14, 12, 16)],
        &[(3, 3, " "), (10, 10, " ")],
    );
    row(
        "insertion between retained multibyte scalars",
        "é🙂",
        "é 🙂",
        &[(0, 2, 0, 2), (2, 6, 3, 7)],
        &[(2, 2, " ")],
    );
}

/// Insert, delete, replace, Unicode and newline cases apply
/// exactly.
#[test]
fn table_cases_apply_exactly_for_every_edit_kind() {
    row(
        "identical bytes fully anchored",
        "ab",
        "ab",
        &[(0, 1, 0, 1), (1, 2, 1, 2)],
        &[],
    );
    row("identical bytes without anchors", "ab", "ab", &[], &[]);
    row("empty authored and rendered", "", "", &[], &[]);
    row(
        "insert into an empty source",
        "",
        "new",
        &[],
        &[(0, 0, "new")],
    );
    row("delete to an empty result", "old", "", &[], &[(0, 3, "")]);
    row(
        "deletion between anchors",
        "a  b",
        "a b",
        &[(0, 1, 0, 1), (3, 4, 2, 3)],
        &[(2, 3, "")],
    );
    row(
        "replacement between anchors",
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
        "whole replacement without anchors",
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
        "final newline insertion at EOF",
        "x",
        "x\n",
        &[(0, 1, 0, 1)],
        &[(1, 1, "\n")],
    );
    row(
        "BOM deletion keeps the retained tail",
        "\u{feff}x",
        "x",
        &[(3, 4, 0, 1)],
        &[(0, 3, "")],
    );
    row(
        "CRLF to LF replaces each whole pair",
        "a\r\nb\r\n",
        "a\nb\n",
        &[(0, 1, 0, 1), (3, 4, 2, 3)],
        &[(1, 3, "\n"), (4, 6, "\n")],
    );
    row(
        "LF to CRLF inserts each carriage return",
        "a\nb\n",
        "a\r\nb\r\n",
        &[(0, 1, 0, 1), (2, 3, 3, 4)],
        &[(1, 1, "\r"), (3, 3, "\r")],
    );
    row(
        "CR to LF replaces each terminator",
        "a\rb\r",
        "a\nb\n",
        &[(0, 1, 0, 1), (2, 3, 2, 3)],
        &[(1, 2, "\n"), (3, 4, "\n")],
    );
    // An insertion between the `\r` and the `\n` never yields an edit
    // boundary inside the pair: the pair is replaced as a whole.
    row(
        "insertion inside a CRLF pair replaces the whole pair",
        "a\r\nb",
        "a\rX\nb",
        &[],
        &[(1, 3, "\rX\n")],
    );
    // A binding whose boundary splits the pair cannot anchor: it stays gap
    // content and the pair is still replaced as a whole.
    row(
        "binding that splits a CRLF pair stays gap content",
        "a\r\nb",
        "a\nb",
        &[(0, 1, 0, 1), (2, 3, 1, 2), (3, 4, 2, 3)],
        &[(1, 3, "\n")],
    );
    row(
        "multibyte replacement at scalar boundaries",
        "é🙂",
        "🙂é",
        &[],
        &[(0, 6, "🙂é")],
    );
    // Scalars that share a leading byte are compared as scalars, so the
    // boundary never lands inside a sequence.
    row(
        "shared lead byte is not a shared scalar",
        "é",
        "è",
        &[],
        &[(0, 2, "è")],
    );
    row(
        "combining mark insertion after a retained base",
        "e!",
        "e\u{301}!",
        &[(0, 1, 0, 1), (1, 2, 3, 4)],
        &[(1, 1, "\u{301}")],
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
fn unequal_or_unordered_bindings_do_not_fabricate_anchors() {
    // The bound bytes differ, so the unit is gap content, not a stable
    // anchor; derivation still applies exactly.
    row(
        "transformed binding becomes one replacement",
        "abc",
        "aXc",
        &[(0, 2, 0, 2)],
        &[(1, 2, "X")],
    );
    // Equal bytes bound out of authored order are a move, not a bijection:
    // the later unit is gap content, expressed as delete plus insert.
    row(
        "moved retained unit is delete plus insert",
        "ab",
        "ba",
        &[(1, 2, 0, 1), (0, 1, 1, 2)],
        &[(0, 1, ""), (2, 2, "a")],
    );
    // A binding that is shorter in the output than its authored range (a
    // truncation) stays gap content and the lost bytes are a deletion.
    row(
        "shortened binding is a deletion",
        "abc",
        "ab",
        &[(0, 3, 0, 2)],
        &[(2, 3, "")],
    );
}

#[test]
fn adjacent_gaps_coalesce_only_when_no_retained_byte_separates_them() {
    // A zero-width unit in both spaces separates nothing: the two generated
    // runs coalesce into one insertion, in rendered order.
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
    // Only a retained unit separates gaps. An equal byte inside one gap that
    // no binding retains is part of the replacement: the algorithm strips a
    // gap's edges and never searches its interior.
    row(
        "unretained interior byte stays inside one replacement",
        "aXbYc",
        "a1b2c",
        &[(0, 1, 0, 1), (4, 5, 4, 5)],
        &[(1, 4, "1b2")],
    );
    // The same byte, retained by a binding, splits it in two.
    row(
        "retained interior byte splits the replacement",
        "aXbYc",
        "a1b2c",
        &[(0, 1, 0, 1), (2, 3, 2, 3), (4, 5, 4, 5)],
        &[(1, 2, "1"), (3, 4, "2")],
    );
}

/// Unchanged prefix, suffix and internal stable regions are
/// excluded; a whole-file replacement is never emitted when the locked
/// algorithm proves a smaller set.
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

    // The locked example proves the internal stable region: two insertions,
    // not one whole-file replacement.
    let authored = "a =1;\r\nb =2;\r\n";
    let set = derive_case(
        authored,
        "a = 1;\r\nb = 2;\r\n",
        &[(0, 3, 0, 3), (3, 10, 4, 11), (10, 14, 12, 16)],
    )
    .unwrap();
    assert_eq!(set.len(), 2);
    assert!(set
        .edits()
        .iter()
        .all(|edit| edit.range.len() < authored.len() as u32));

    // Anchored head and tail with one changed middle: only the middle edits.
    let set = derive_case(
        "headMIDtail",
        "headNEWtail",
        &[(0, 4, 0, 4), (7, 11, 7, 11)],
    )
    .unwrap();
    assert_well_formed(&set, "headMIDtail");
    assert_eq!(tuples(&set), vec![(4, 7, "NEW".to_owned())]);

    // A byte-identical result derives the empty set: the second pass changes
    // nothing (`edits.second_pass`), however the segments claim their
    // bindings, even as a swap of two equal units that is no bijection.
    let same = derive_case(authored, authored, &[(0, 7, 0, 7), (7, 14, 7, 14)]).unwrap();
    assert!(same.is_empty());
    assert_eq!(same.apply(authored).unwrap(), authored);
    let swapped = derive_case("aa", "aa", &[(1, 2, 0, 1), (0, 1, 1, 2)]).unwrap();
    assert!(swapped.is_empty());
    assert_well_formed(&swapped, "aa");
    // Identical bytes still refuse a binding that is not addressable.
    assert_eq!(
        derive_edits("aa", "aa", &tile("aaa", "aa", &[(0, 3, 0, 2)])),
        Err(EditError::InvalidRetainedRange {
            index: 0,
            error: CoordinateError::OutOfBounds {
                offset: 3,
                source_len: 2
            }
        })
    );
}

/// Edits derived from a rendered document reproduce it exactly and never
/// intersect a retained unit; a document that reproduces its source derives
/// zero edits.
#[test]
fn derivation_from_a_rendered_doc_applies_exactly() {
    let authored = "let x=1;\nlet  y=2;\n";
    let mut table = FormatProvenanceTable::new(41_u32);
    let bind = |table: &mut FormatProvenanceTable<u32>, start: usize, end: usize| {
        let range = authored_range(authored, start, end);
        Provenanced::new(
            authored[start..end].to_owned(),
            AuthoredProvenance::new(table.bind_authored(range), range),
        )
    };
    let mut doc = Doc::new();
    let let_x = doc.source(bind(&mut table, 0, 5));
    let eq1 = doc.text(" = ");
    let one = doc.source(bind(&mut table, 6, 8));
    let nl1 = doc.line(Line::Hard);
    let let_ = doc.source(bind(&mut table, 9, 12));
    let space = doc.text(" ");
    let y = doc.source(bind(&mut table, 14, 15));
    let eq2 = doc.text(" = ");
    let two = doc.source(bind(&mut table, 16, 18));
    let nl2 = doc.line(Line::Hard);
    let root = doc.concat([let_x, eq1, one, nl1, let_, space, y, eq2, two, nl2]);
    let rendered = render(&doc, root, RenderOptions::default()).unwrap();
    assert_eq!(rendered.text, "let x = 1;\nlet y = 2;\n");
    let set = derive_edits_from_doc(authored, &rendered).unwrap();
    assert_well_formed(&set, authored);
    // The retained newline between the lines and the equal first space of
    // the doubled one are stripped; only the three changed islands edit.
    assert_eq!(
        tuples(&set),
        vec![
            (5, 6, " = ".to_owned()),
            (13, 14, String::new()),
            (15, 16, " = ".to_owned()),
        ]
    );
    assert_eq!(set.apply(authored).unwrap(), rendered.text);
    for anchor in table.iter() {
        for edit in set.edits() {
            let (a0, a1) = (anchor.range().start().get(), anchor.range().end().get());
            let (e0, e1) = (edit.range.start().get(), edit.range.end().get());
            assert!(
                e1 <= a0 || e0 >= a1,
                "edit {e0}..{e1} intersects retained unit {a0}..{a1}"
            );
        }
    }

    // CRLF output from LF source through the renderer's configured ending:
    // each inserted `\r` is its own edit and no boundary splits a pair.
    let lines = |doc: &mut Doc, table: &mut FormatProvenanceTable<u32>| {
        let first = doc.source(bind(table, 0, 8));
        let nl1 = doc.line(Line::Hard);
        let second = doc.source(bind(table, 9, 18));
        let nl2 = doc.line(Line::Hard);
        doc.concat([first, nl1, second, nl2])
    };
    let mut doc = Doc::new();
    let root = lines(&mut doc, &mut table);
    let rendered = render(
        &doc,
        root,
        RenderOptions {
            line_ending: LineEnding::CrLf,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(rendered.text, "let x=1;\r\nlet  y=2;\r\n");
    let set = derive_edits_from_doc(authored, &rendered).unwrap();
    assert_well_formed(&set, authored);
    assert_eq!(
        tuples(&set),
        vec![(8, 8, "\r".to_owned()), (18, 18, "\r".to_owned())]
    );
    assert_eq!(set.apply(authored).unwrap(), rendered.text);

    // A document that reproduces its source byte for byte derives nothing.
    let mut doc = Doc::new();
    let root = lines(&mut doc, &mut table);
    let same = render(&doc, root, RenderOptions::default()).unwrap();
    assert_eq!(same.text, authored);
    assert!(derive_edits_from_doc(authored, &same).unwrap().is_empty());
}

/// The ratified performance catalog, so the enforced `edit_comparisons`
/// ceiling is read from its lock and a changed lock fails this test instead
/// of passing against a stale copy of the number.
const PERFORMANCE_CATALOG: &str =
    include_str!("../../../../catalogs/formatter-performance-v1.toml");

fn assert_within_locked_work(set: &FormatEditSet, authored: &str, rendered: &str) -> usize {
    let locked = PERFORMANCE_CATALOG
        .lines()
        .zip(PERFORMANCE_CATALOG.lines().skip(1))
        .find_map(|(counter, ceiling)| {
            (counter.trim() == "counter = \"edit_comparisons\"")
                .then(|| ceiling.trim().strip_prefix("ceiling = "))
                .flatten()
                .map(|value| value.trim_matches('"'))
        })
        .expect("edit_comparisons bound locked in the performance catalog");
    assert_eq!(
        locked, "4*(A_in+B_emit)+64",
        "the enforced ceiling no longer matches the ratified edit_comparisons lock"
    );
    let ceiling = 4 * (authored.len() + rendered.len()) + 64;
    let comparisons = set.work().edit_comparisons;
    assert!(
        comparisons <= ceiling,
        "edit_comparisons {comparisons} exceeds the locked ceiling {ceiling}"
    );
    let emitted: usize = set.edits().iter().map(|edit| edit.text.len()).sum();
    assert!(
        emitted <= rendered.len(),
        "edit text bytes {emitted} exceed the rendered size"
    );
    comparisons
}

/// Alternating bytes with nothing in common: prefix and suffix stripping
/// each stop at the first scalar, so comparison work is constant in the size.
fn adversary_alternating(size: usize) -> usize {
    let authored = "ab".repeat(size);
    let rendered = "ba".repeat(size);
    let set = derive_case(&authored, &rendered, &[]).unwrap();
    assert_well_formed(&set, &authored);
    assert_eq!(set.apply(&authored).unwrap(), rendered);
    assert_within_locked_work(&set, &authored, &rendered)
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
    assert_within_locked_work(&set, &authored, &rendered)
}

/// Anchor-dense layout: every retained byte alternates with a generated
/// byte, maximizing gaps, anchors and replacements per byte.
fn adversary_anchor_dense(size: usize) -> usize {
    let authored = "a".repeat(size);
    let rendered: String = (0..size)
        .map(|i| if i % 2 == 0 { 'a' } else { 'X' })
        .collect();
    let anchors: Vec<_> = (0..size).step_by(2).map(|i| (i, i + 1, i, i + 1)).collect();
    let set = derive_case(&authored, &rendered, &anchors).unwrap();
    assert_well_formed(&set, &authored);
    assert_eq!(set.apply(&authored).unwrap(), rendered);
    assert_within_locked_work(&set, &authored, &rendered)
}

/// Repeated near-equal lines: a global diff would search quadratically for
/// the best alignment; the locked algorithm strips one prefix and one
/// suffix and emits one interior replacement.
fn adversary_repetitive(size: usize) -> usize {
    let line = "const value = 1;\n";
    let authored = line.repeat(size);
    let mut rendered = line.repeat(size / 2);
    rendered.push_str("const value = 2;\n");
    rendered.push_str(&line.repeat(size - size / 2 - 1));
    let set = derive_case(&authored, &rendered, &[]).unwrap();
    assert_well_formed(&set, &authored);
    assert_eq!(set.len(), 1);
    assert_eq!(set.apply(&authored).unwrap(), rendered);
    assert_within_locked_work(&set, &authored, &rendered)
}

/// Adversarial pairs stay within the locked work bound and grow
/// linearly, with no quadratic fallback.
#[test]
fn adversarial_pairs_stay_within_the_locked_work_bounds() {
    let (a1, a2) = (adversary_alternating(1000), adversary_alternating(2000));
    assert!(a2 <= a1 + 4, "no common structure must stay constant work");
    let (p1, p2) = (adversary_long_prefix(1000), adversary_long_prefix(2000));
    assert!(p2 <= 2 * p1 + 64, "prefix stripping must stay linear");
    let (d1, d2) = (adversary_anchor_dense(1000), adversary_anchor_dense(2000));
    assert!(d2 <= 2 * d1 + 64, "anchor verification must stay linear");
    let (r1, r2) = (adversary_repetitive(1000), adversary_repetitive(2000));
    assert!(r2 <= 2 * r1 + 64, "repetitive content must stay linear");
}

mod nightly_scale {
    #[test]
    fn adversarial_pairs_at_scale() {
        assert!(super::adversary_alternating(1_000_000) <= 8);
        super::adversary_long_prefix(1_000_000);
        super::adversary_anchor_dense(1_000_000);
        super::adversary_repetitive(100_000);
    }
}

#[test]
fn malformed_rendered_segments_and_bindings_are_refused() {
    let authored = "ab";
    let rendered = "ab";
    let mut table = FormatProvenanceTable::new(0_u32);
    // Segments must tile the rendered text exactly: a hole, an overlap and a
    // short cover each name the failing segment.
    let hole = [
        generated(rendered, 0, 1),
        source(&mut table, rendered, (2, 2), authored_range(authored, 1, 2)),
    ];
    assert_eq!(
        derive_edits(authored, rendered, &hole),
        Err(EditError::SegmentsNotContiguous { index: 1 })
    );
    let overlap = [generated(rendered, 0, 2), generated(rendered, 1, 2)];
    assert_eq!(
        derive_edits(authored, rendered, &overlap),
        Err(EditError::SegmentsNotContiguous { index: 1 })
    );
    let short = [generated(rendered, 0, 1)];
    assert_eq!(
        derive_edits(authored, rendered, &short),
        Err(EditError::SegmentsNotContiguous { index: 1 })
    );
    assert_eq!(
        derive_edits(authored, rendered, &[]),
        Err(EditError::SegmentsNotContiguous { index: 0 })
    );
    // A binding minted against another text is refused against this one:
    // past the end, or inside a multibyte scalar.
    let foreign = tile("abcdef", rendered, &[(0, 5, 0, 2)]);
    assert_eq!(
        derive_edits(authored, rendered, &foreign),
        Err(EditError::InvalidRetainedRange {
            index: 0,
            error: CoordinateError::OutOfBounds {
                offset: 5,
                source_len: 2
            }
        })
    );
    let inside = tile("ab", "ab", &[(0, 1, 0, 1)]);
    assert_eq!(
        derive_edits("é🙂", "ab", &inside),
        Err(EditError::InvalidRetainedRange {
            index: 0,
            error: CoordinateError::NotCharBoundary { offset: 1 }
        })
    );
    // Rendered ranges minted against another text of the same byte length
    // are refused instead of slicing the supplied text at a non-boundary.
    let other = tile(authored, "xy", &[(0, 1, 0, 1)]);
    assert_eq!(
        derive_edits(authored, "é", &other),
        Err(EditError::InvalidRenderedRange {
            index: 0,
            error: CoordinateError::NotCharBoundary { offset: 1 }
        })
    );
    assert_eq!(
        derive_edits(authored, "x", &other),
        Err(EditError::InvalidRenderedRange {
            index: 1,
            error: CoordinateError::OutOfBounds {
                offset: 2,
                source_len: 1
            }
        })
    );
}

/// At the construction boundary, a malformed set cannot be built,
/// and a set applied to a text where one of its edits does not fit is refused.
#[test]
fn construction_and_application_refuse_malformed_edit_sets() {
    let authored = "abcdef";
    let edit = |start, end, text: &str| FormatEdit {
        range: authored_range(authored, start, end),
        text: text.to_owned(),
    };
    let refused = |edits: Vec<FormatEdit>, expected: EditError| {
        assert_eq!(FormatEditSet::new(authored, edits).unwrap_err(), expected);
    };
    refused(
        vec![edit(5, 6, "x"), edit(1, 2, "y")],
        EditError::EditsOutOfOrder { index: 1 },
    );
    refused(
        vec![edit(0, 2, "x"), edit(1, 3, "y")],
        EditError::EditsOutOfOrder { index: 1 },
    );
    refused(vec![edit(3, 3, "")], EditError::NoOpEdit { index: 0 });
    refused(vec![edit(0, 2, "ab")], EditError::NoOpEdit { index: 0 });
    // Neither endpoint may fall between the `\r` and the `\n` of a pair
    // (`maps.crlf`), at construction or at application.
    let crlf = "ab\r\ncd";
    let pair_edit = |start, end, text: &str| FormatEdit {
        range: authored_range(crlf, start, end),
        text: text.to_owned(),
    };
    assert_eq!(
        FormatEditSet::new(crlf, vec![pair_edit(3, 3, "X")]).unwrap_err(),
        EditError::EditSplitsCrlfPair { index: 0 }
    );
    assert_eq!(
        FormatEditSet::new(crlf, vec![pair_edit(1, 3, "Z")]).unwrap_err(),
        EditError::EditSplitsCrlfPair { index: 0 }
    );
    assert_eq!(
        FormatEditSet::new(crlf, vec![pair_edit(2, 4, "\n")])
            .unwrap()
            .apply(crlf)
            .unwrap(),
        "ab\ncd"
    );
    let insert = FormatEditSet::new(authored, vec![edit(3, 3, "X")]).unwrap();
    assert_eq!(
        insert.apply(crlf),
        Err(EditError::EditSplitsCrlfPair { index: 0 })
    );
    // A range minted against a longer text is refused against this source.
    let foreign = FormatEdit {
        range: authored_range("abcdefghij", 9, 10),
        text: "x".to_owned(),
    };
    refused(
        vec![foreign],
        EditError::InvalidEdit {
            index: 0,
            error: CoordinateError::OutOfBounds {
                offset: 10,
                source_len: 6,
            },
        },
    );

    // Insertions at one boundary keep their stored (rendered) order; touching
    // edits are non-overlapping.
    let set = FormatEditSet::new(
        authored,
        vec![edit(3, 3, "X"), edit(3, 3, "Y"), edit(3, 4, "")],
    )
    .unwrap();
    assert_eq!(set.apply(authored).unwrap(), "abcXYef");

    // A set is bound to its source: the same edits do not apply to a text
    // where a range lands inside a scalar or where they would be no-ops.
    assert_eq!(
        set.apply("ab🙂ef"),
        Err(EditError::InvalidEdit {
            index: 0,
            error: CoordinateError::NotCharBoundary { offset: 3 }
        })
    );
    let replace = FormatEditSet::new(authored, vec![edit(2, 3, "X")]).unwrap();
    assert_eq!(
        replace.apply("abXdef"),
        Err(EditError::NoOpEdit { index: 0 })
    );
    let empty = FormatEditSet::new(authored, Vec::new()).unwrap();
    assert!(empty.is_empty());
    assert_eq!(empty.apply(authored).unwrap(), authored);
}

#[test]
fn errors_display_their_position() {
    assert_eq!(
        EditError::SegmentsNotContiguous { index: 3 }.to_string(),
        "rendered segment 3 does not tile the rendered text"
    );
    assert_eq!(
        EditError::NoOpEdit { index: 1 }.to_string(),
        "edit 1 replaces nothing or identical bytes"
    );
    assert_eq!(
        EditError::EditSplitsCrlfPair { index: 2 }.to_string(),
        "edit 2 has a boundary inside a CRLF pair"
    );
    assert_eq!(
        EditError::InvalidEdit {
            index: 0,
            error: CoordinateError::NotCharBoundary { offset: 3 }
        }
        .to_string(),
        "edit 0 is invalid for its source: offset 3 is inside a multi-byte UTF-8 sequence"
    );
}
