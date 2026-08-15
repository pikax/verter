//! Geometry tests for [`CodeTransform::chain_source_map`].
//!
//! Each case names the property it pins. Where a case has a hand-derivable
//! answer the derivation is in the doc comment, so a failure says which rule
//! broke rather than only which number moved.

use oxc_allocator::Allocator;
use oxc_sourcemap::{SourceMap, Token};

use crate::code_transform::{advance_generated_position, CodeTransform, SourceMapChainError};

/// `(genLine, genCol, srcLine, srcCol, srcIdx, nameIdx)`, with the four
/// authored fields absent for a sourceless segment.
type Seg = (u32, u32, Option<(u32, u32, u32, Option<u32>)>);

fn source_bearing(line: u32, column: u32, src_line: u32, src_col: u32) -> Token {
    Token::new(line, column, src_line, src_col, Some(0), None)
}

fn sourceless(line: u32, column: u32) -> Token {
    Token::new(line, column, 0, 0, None, None)
}

fn map_of(tokens: Vec<Token>) -> SourceMap<'static> {
    SourceMap::new(
        None,
        Vec::new(),
        None,
        vec!["Comp.vue".into()],
        vec![None],
        tokens.into_boxed_slice(),
        None,
    )
}

fn segments(map: &SourceMap<'_>) -> Vec<Seg> {
    map.get_tokens()
        .map(|token| {
            (
                token.get_dst_line(),
                token.get_dst_col(),
                token.get_source_id().map(|source_id| {
                    (
                        token.get_src_line(),
                        token.get_src_col(),
                        source_id,
                        token.get_name_id(),
                    )
                }),
            )
        })
        .collect()
}

/// Every literal, non-overlapping, left-to-right occurrence of `needle`, the
/// same match set `str::replace` visits.
fn occurrences(haystack: &str, needle: &str) -> Vec<u32> {
    let mut found = Vec::new();
    let mut from = 0usize;
    while let Some(relative) = haystack[from..].find(needle) {
        let at = from + relative;
        found.push(at as u32);
        from = at + needle.len();
    }
    found
}

/// Apply a global literal rewrite and chain `input` through it, returning the
/// rewritten code and the chained segments.
fn rewrite_and_chain(
    code: &str,
    pattern: &str,
    replacement: &str,
    input: &SourceMap<'_>,
) -> (String, Vec<Seg>) {
    let allocator = Allocator::default();
    let mut transform = CodeTransform::new(code, &allocator);
    for start in occurrences(code, pattern) {
        transform.overwrite(start, start + pattern.len() as u32, replacement);
    }
    let chained = transform
        .chain_source_map(input)
        .expect("a transform built from overwrites alone chains");
    (transform.build_string(), segments(&chained))
}

// ── The two authorized rewrites, end to end ────────────────────────────────

/// A terminal removal has no chunk after it, so it produces no resume segment
/// at all — only the rename's own geometry survives.
#[test]
fn terminal_removal_emits_no_resume_segment() {
    let code = "const __sfc__ = {}\nexport default __sfc__;\n";
    let input = map_of(vec![
        source_bearing(0, 0, 1, 0),
        source_bearing(0, 6, 1, 6),
        source_bearing(1, 0, 2, 0),
        source_bearing(1, 15, 2, 15),
    ]);

    let (renamed, _) = rewrite_and_chain(code, "__sfc__", "_sfc_main", &input);
    let allocator = Allocator::default();
    let mut pass_one = CodeTransform::new(code, &allocator);
    for start in occurrences(code, "__sfc__") {
        pass_one.overwrite(start, start + 7, "_sfc_main");
    }
    let after_one = pass_one.chain_source_map(&input).expect("pass one chains");

    let mut pass_two = CodeTransform::new(&renamed, &allocator);
    for start in occurrences(&renamed, "export default _sfc_main;\n") {
        pass_two.overwrite(start, start + 26, "");
    }
    let after_two = pass_two
        .chain_source_map(&after_one)
        .expect("pass two chains");

    assert_eq!(pass_two.build_string(), "const _sfc_main = {}\n");
    assert_eq!(
        segments(&after_two),
        vec![
            (0, 0, Some((1, 0, 0, None))),
            (0, 6, Some((1, 6, 0, None))),
            (0, 15, Some((1, 6, 0, None))),
        ],
        "line 1 is entirely inside the removed range and the removal is \
         terminal, so nothing follows it"
    );
}

/// A NON-terminal removal is followed by surviving text, whose chunk start is a
/// resume point. That resume is SOURCELESS here: the only segment on its
/// resolved line sits to its right, and the lookup is line-scoped rather than
/// falling through to the previous line.
#[test]
fn non_terminal_removal_resumes_sourcelessly_when_its_line_has_nothing_left_of_it() {
    let code = "const __sfc__ = {}\nexport default __sfc__;\nconst tail = 1\n";
    let input = map_of(vec![
        source_bearing(0, 0, 1, 0),
        source_bearing(0, 6, 1, 6),
        source_bearing(1, 0, 2, 0),
        source_bearing(1, 15, 2, 15),
        source_bearing(2, 6, 3, 6),
    ]);

    let allocator = Allocator::default();
    let mut pass_one = CodeTransform::new(code, &allocator);
    for start in occurrences(code, "__sfc__") {
        pass_one.overwrite(start, start + 7, "_sfc_main");
    }
    let renamed = pass_one.build_string();
    let after_one = pass_one.chain_source_map(&input).expect("pass one chains");

    let mut pass_two = CodeTransform::new(&renamed, &allocator);
    for start in occurrences(&renamed, "export default _sfc_main;\n") {
        pass_two.overwrite(start, start + 26, "");
    }
    let after_two = pass_two
        .chain_source_map(&after_one)
        .expect("pass two chains");

    assert_eq!(
        pass_two.build_string(),
        "const _sfc_main = {}\nconst tail = 1\n"
    );
    assert_eq!(
        segments(&after_two),
        vec![
            (0, 0, Some((1, 0, 0, None))),
            (0, 6, Some((1, 6, 0, None))),
            (0, 15, Some((1, 6, 0, None))),
            (1, 0, None),
            (1, 6, Some((3, 6, 0, None))),
        ]
    );
    assert!(
        segments(&after_two)[3].2.is_none(),
        "a global rather than line-scoped lookup would wrongly inherit \
         authored (2,15) here"
    );
}

// ── Equal coordinates, ordering, and collision policy ──────────────────────

/// Two segments at one coordinate keep their declared order through a chain
/// that touches neither. A multiset or column-sorted comparison cannot tell
/// this apart from its swap, but the accepted lookup can: it takes the LAST.
#[test]
fn coincident_segments_keep_declared_order() {
    let code = "const x = 1\n";
    let input = map_of(vec![source_bearing(0, 0, 1, 0), source_bearing(0, 0, 5, 5)]);

    let (built, chained) = rewrite_and_chain(code, "__sfc__", "_sfc_main", &input);

    assert_eq!(built, code, "no occurrence, so the pass is the identity");
    assert_eq!(
        chained,
        vec![(0, 0, Some((1, 0, 0, None))), (0, 0, Some((5, 5, 0, None)))]
    );
}

/// A replaced range beginning at column 0 with N coincident prior segments:
/// all N are dropped, exactly ONE replacement segment is emitted, and it
/// carries the LAST of them — then the resume follows at the replacement's end.
#[test]
fn replacement_drops_every_coincident_segment_and_carries_the_last() {
    let code = "__sfc__ = 1\n";
    let input = map_of(vec![
        source_bearing(0, 0, 1, 0),
        source_bearing(0, 0, 2, 2),
        source_bearing(0, 0, 3, 3),
    ]);

    let (built, chained) = rewrite_and_chain(code, "__sfc__", "_sfc_main", &input);

    assert_eq!(built, "_sfc_main = 1\n");
    assert_eq!(
        chained,
        vec![(0, 0, Some((3, 3, 0, None))), (0, 9, Some((3, 3, 0, None))),],
        "one replacement segment carrying the third input segment, then the \
         resume at the replacement's end"
    );
}

/// When the resume offset already carries input segments, the resume is
/// suppressed: it would have been a byte-identical duplicate of the last of
/// them, because the lookup takes exactly that segment.
#[test]
fn resume_is_suppressed_when_the_resume_offset_carries_input_segments() {
    let code = "__sfc__x\n";
    let input = map_of(vec![source_bearing(0, 0, 1, 0), source_bearing(0, 7, 4, 4)]);

    let (built, chained) = rewrite_and_chain(code, "__sfc__", "_sfc_main", &input);

    assert_eq!(built, "_sfc_mainx\n");
    assert_eq!(
        chained,
        vec![(0, 0, Some((1, 0, 0, None))), (0, 9, Some((4, 4, 0, None))),],
        "exactly one segment at the resume coordinate, not a duplicate pair"
    );
}

/// Two replacements on one line each get their own replacement segment and
/// their own resume segment.
#[test]
fn multiple_same_line_replacements_each_get_a_replacement_and_a_resume() {
    let code = "a __sfc__ b __sfc__ c\n";
    let input = map_of(vec![source_bearing(0, 0, 1, 0)]);

    let (built, chained) = rewrite_and_chain(code, "__sfc__", "_sfc_main", &input);

    // Occurrences sit at input offsets 2 and 12. In the OUTPUT the first
    // replacement occupies columns 2..11, so " b " lands at 11..14, the second
    // replacement at 14..23, and the text resuming after it at 23.
    assert_eq!(built, "a _sfc_main b _sfc_main c\n");
    assert_eq!(
        chained,
        vec![
            (0, 0, Some((1, 0, 0, None))),
            (0, 2, Some((1, 0, 0, None))),
            (0, 11, Some((1, 0, 0, None))),
            (0, 14, Some((1, 0, 0, None))),
            (0, 23, Some((1, 0, 0, None))),
        ]
    );
}

/// Two distinct segments strictly INSIDE a replaced range are both dropped
/// from the OUTPUT — but they remain part of the input sequence the lookup
/// reads. So the replacement segment carries the segment at or before the
/// range's start, while the resume carries the last one at or before the
/// range's END, which here is an interior segment that was itself dropped.
///
/// Removing dropped segments from the lookup instead would make the resume
/// carry authored (1,0), and would break the rename's own base case, where the
/// replaced range's start IS a declared segment.
#[test]
fn segments_strictly_inside_a_replaced_range_are_dropped_but_still_resolve() {
    let code = "x __sfc__ y\n";
    let input = map_of(vec![
        source_bearing(0, 0, 1, 0),
        source_bearing(0, 3, 7, 7),
        source_bearing(0, 5, 8, 8),
    ]);

    let (built, chained) = rewrite_and_chain(code, "__sfc__", "_sfc_main", &input);

    assert_eq!(built, "x _sfc_main y\n");
    assert_eq!(
        chained,
        vec![
            (0, 0, Some((1, 0, 0, None))),
            (0, 2, Some((1, 0, 0, None))),
            (0, 11, Some((8, 8, 0, None))),
        ],
        "exactly three segments: neither interior segment survives in its own \
         right, and the resume at old column 9 resolves to the last of them"
    );
}

// ── The sourceless barrier ─────────────────────────────────────────────────

/// A sourceless segment is a legitimate lookup RESULT, not a hole to see
/// through. Both lookups here land after it and both stay sourceless; an
/// implementation that skipped it would fabricate authored (1,0) twice.
#[test]
fn a_sourceless_segment_is_a_barrier_at_every_lookup_after_it() {
    let code = "const __sfc__ = {}\n";
    let input = map_of(vec![source_bearing(0, 0, 1, 0), sourceless(0, 3)]);

    let (built, chained) = rewrite_and_chain(code, "__sfc__", "_sfc_main", &input);

    assert_eq!(built, "const _sfc_main = {}\n");
    assert_eq!(
        chained,
        vec![
            (0, 0, Some((1, 0, 0, None))),
            (0, 3, None),
            (0, 6, None),
            (0, 15, None),
        ]
    );
}

/// A resume whose own line declares nothing at or before it emits a SOURCELESS
/// segment rather than nothing, and rather than inheriting a previous LINE.
///
/// The removal here consumes the newline ending input line 0, so the two lines
/// MERGE: the resume's output position is mid-line, to the right of a
/// source-bearing segment, while its INPUT position is `(1, 0)` — a line whose
/// only segment sits to its right. Emitting nothing would let the surviving
/// text inherit the segment to its left in the merged output; falling through
/// to input line 0 would inherit authored (1,0). Both fabricate provenance.
#[test]
fn a_resume_with_no_applicable_segment_emits_sourceless_rather_than_nothing() {
    let code = "ab__sfc__\ncd\n";
    let input = map_of(vec![source_bearing(0, 0, 1, 0), source_bearing(1, 1, 9, 9)]);

    let allocator = Allocator::default();
    let mut transform = CodeTransform::new(code, &allocator);
    // Remove "__sfc__\n" — a removal that consumes the newline.
    transform.overwrite(2, 10, "");
    let chained = transform
        .chain_source_map(&input)
        .expect("an overwrite-only transform chains");

    assert_eq!(transform.build_string(), "abcd\n");
    assert_eq!(
        segments(&chained),
        vec![
            (0, 0, Some((1, 0, 0, None))),
            (0, 2, None),
            (0, 3, Some((9, 9, 0, None))),
        ]
    );
    assert!(
        segments(&chained)[1].2.is_none(),
        "the resume must not inherit input line 0's segment across the merge"
    );
}

// ── UTF-16 columns ─────────────────────────────────────────────────────────

/// Columns are UTF-16 code units — not code points, and not UTF-8 bytes. The
/// occurrence below sits at unit 11, code point 10, and byte 13.
#[test]
fn columns_are_utf16_code_units_not_code_points_or_bytes() {
    let code = "const \u{1D400} = __sfc__\n";
    let input = map_of(vec![
        source_bearing(0, 0, 1, 0),
        source_bearing(0, 11, 1, 11),
    ]);

    assert_eq!(code.find("__sfc__"), Some(13), "the UTF-8 byte offset");
    assert_eq!(
        code.chars().take_while(|c| *c != '_').count(),
        10,
        "the code-point index"
    );

    let (built, chained) = rewrite_and_chain(code, "__sfc__", "_sfc_main", &input);

    assert_eq!(built, "const \u{1D400} = _sfc_main\n");
    assert_eq!(
        chained,
        vec![
            (0, 0, Some((1, 0, 0, None))),
            (0, 11, Some((1, 11, 0, None))),
            (0, 20, Some((1, 11, 0, None))),
        ]
    );
}

/// A retained CR occupies a real column: lines split on LF only.
#[test]
fn a_retained_carriage_return_occupies_a_column() {
    let code = "const a = 1\r\nconst __sfc__ = {}\r\n";
    let input = map_of(vec![
        source_bearing(0, 0, 1, 0),
        source_bearing(1, 6, 2, 6),
        // The CR of line 0, at column 11 — only reachable if the CR is retained
        // in the line's text rather than stripped with the newline.
        source_bearing(0, 11, 1, 11),
    ]);

    let (built, chained) = rewrite_and_chain(code, "__sfc__", "_sfc_main", &input);

    assert_eq!(built, "const a = 1\r\nconst _sfc_main = {}\r\n");
    assert_eq!(
        chained,
        vec![
            (0, 0, Some((1, 0, 0, None))),
            (0, 11, Some((1, 11, 0, None))),
            (1, 6, Some((2, 6, 0, None))),
            (1, 15, Some((2, 6, 0, None))),
        ],
        "the CR column survives, and the declared order of the two line-0 \
         segments is restored to generated order by their offsets"
    );
}

/// A column that would split a surrogate pair addresses no byte offset and is
/// refused rather than rounded to either side.
#[test]
fn a_column_splitting_a_surrogate_pair_is_refused() {
    let code = "\u{1D400}x\n";
    let input = map_of(vec![source_bearing(0, 1, 1, 0)]);

    let allocator = Allocator::default();
    let transform = CodeTransform::new(code, &allocator);

    assert_eq!(
        transform.chain_source_map(&input).unwrap_err(),
        SourceMapChainError::SegmentPositionOutOfBounds { line: 0, column: 1 }
    );
}

// ── End-of-text segments ───────────────────────────────────────────────────

/// A segment one past the last byte is covered by no chunk. It is carried at
/// the output's end position rather than silently dropped.
#[test]
fn a_segment_at_the_end_position_is_carried() {
    let code = "const __sfc__ = {}\n";
    // The trailing empty line's only in-bounds column is 0.
    let input = map_of(vec![source_bearing(0, 0, 1, 0), source_bearing(1, 0, 9, 0)]);

    let (built, chained) = rewrite_and_chain(code, "__sfc__", "_sfc_main", &input);

    assert_eq!(built, "const _sfc_main = {}\n");
    assert_eq!(
        chained,
        vec![
            (0, 0, Some((1, 0, 0, None))),
            (0, 6, Some((1, 0, 0, None))),
            (0, 15, Some((1, 0, 0, None))),
            (1, 0, Some((9, 0, 0, None))),
        ],
        "line 0 declares only (0,0), so both the replacement and the resume \
         resolve to it; the trailing-empty-line segment is carried last"
    );
}

/// The end-position rule is NOT conditioned on a trailing newline: for text
/// that does not end with one, the end position is end-of-line on the last
/// line, and a segment there is still carried.
#[test]
fn the_end_position_rule_is_live_without_a_trailing_newline() {
    let code = "abc";
    let input = map_of(vec![source_bearing(0, 3, 4, 4)]);

    let allocator = Allocator::default();
    let transform = CodeTransform::new(code, &allocator);
    let chained = transform.chain_source_map(&input).expect("chains");

    assert_eq!(segments(&chained), vec![(0, 3, Some((4, 4, 0, None)))]);
}

/// Empty text has no chunks at all, yet `(0, 0)` is a legal position its map
/// may declare — and the only case in which a chunk-less transform emits.
#[test]
fn empty_text_still_carries_a_segment_at_its_one_legal_position() {
    let input = map_of(vec![source_bearing(0, 0, 2, 2)]);

    let allocator = Allocator::default();
    let transform = CodeTransform::new("", &allocator);
    let chained = transform.chain_source_map(&input).expect("chains");

    assert_eq!(transform.build_string(), "");
    assert_eq!(segments(&chained), vec![(0, 0, Some((2, 2, 0, None)))]);
}

/// A whole-text removal leaves empty output; the terminal removal contributes
/// nothing and only an end-position segment can appear.
#[test]
fn a_whole_text_removal_leaves_only_the_end_position_segment() {
    let code = "export default _sfc_main;\n";
    let input = map_of(vec![source_bearing(0, 0, 1, 0), source_bearing(1, 0, 3, 0)]);

    let (built, chained) = rewrite_and_chain(code, "export default _sfc_main;\n", "", &input);

    assert_eq!(built, "");
    assert_eq!(
        chained,
        vec![(0, 0, Some((3, 0, 0, None)))],
        "the line-0 segment is inside the removed range and dropped; the \
         end-position segment survives at the empty output's only position"
    );
}

// ── Pass-through and refusals ──────────────────────────────────────────────

/// Chaining moves coordinates, never identities: every table passes through
/// untouched, including rows no segment references.
#[test]
fn tables_pass_through_untouched() {
    let mut input = SourceMap::new(
        Some("ignored.js".into()),
        vec!["count".into(), "unused".into()],
        Some("/root".into()),
        vec!["a.vue".into(), "b.vue".into()],
        vec![Some("SOURCE A".into()), None],
        vec![Token::new(0, 0, 1, 1, Some(1), Some(0))].into_boxed_slice(),
        None,
    );
    input.set_x_google_ignore_list(vec![1]);

    let allocator = Allocator::default();
    let transform = CodeTransform::new("const x = 1\n", &allocator);
    let chained = transform.chain_source_map(&input).expect("chains");

    assert_eq!(chained.get_names().collect::<Vec<_>>(), ["count", "unused"]);
    assert_eq!(
        chained.get_sources().collect::<Vec<_>>(),
        ["a.vue", "b.vue"]
    );
    assert_eq!(
        chained.get_source_contents().collect::<Vec<_>>(),
        [Some("SOURCE A"), None]
    );
    assert_eq!(chained.get_source_root(), Some("/root"));
    assert_eq!(chained.get_x_google_ignore_list(), Some(&[1u32][..]));
    assert_eq!(chained.get_file(), Some("ignored.js"));
    assert_eq!(segments(&chained), vec![(0, 0, Some((1, 1, 1, Some(0))))]);
}

/// A transform that inserts has no chaining semantics: the inserted bytes
/// correspond to no input position, so it is refused rather than approximated.
#[test]
fn an_inserting_transform_is_refused() {
    let allocator = Allocator::default();
    let mut transform = CodeTransform::new("const x = 1\n", &allocator);
    transform.prepend("// header\n");

    assert_eq!(
        transform.chain_source_map(&map_of(Vec::new())).unwrap_err(),
        SourceMapChainError::IntroPresent
    );

    let mut appended = CodeTransform::new("const x = 1\n", &allocator);
    appended.append("// footer\n");
    assert_eq!(
        appended.chain_source_map(&map_of(Vec::new())).unwrap_err(),
        SourceMapChainError::OutroPresent
    );
}

/// The refusal covers inserted chunks too, not only intro/outro.
#[test]
fn an_inserted_chunk_is_refused() {
    let allocator = Allocator::default();
    let mut transform = CodeTransform::new("const x = 1\n", &allocator);
    transform.append_left(5, "/*!*/");

    assert_eq!(
        transform.chain_source_map(&map_of(Vec::new())).unwrap_err(),
        SourceMapChainError::UnsupportedChunk("Inserted")
    );
}

// ── The shared advance primitive ───────────────────────────────────────────

/// The write cursor an out-of-crate assembler keeps must be the SAME advance
/// the map walk uses, so incremental advancing has to compose.
#[test]
fn advancing_through_pieces_matches_advancing_through_the_whole() {
    let pieces = ["const \u{1D400}", " = 1\n", "", "next\r\n", "tail"];
    let whole: String = pieces.concat();

    let (mut piece_line, mut piece_column) = (0u32, 0u32);
    for piece in pieces {
        advance_generated_position(piece, &mut piece_line, &mut piece_column);
    }

    let (mut whole_line, mut whole_column) = (0u32, 0u32);
    advance_generated_position(&whole, &mut whole_line, &mut whole_column);

    assert_eq!((piece_line, piece_column), (whole_line, whole_column));
    assert_eq!((whole_line, whole_column), (2, 4));
}
