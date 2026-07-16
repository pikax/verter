//! Unit tests for the strict in-run [`super::PositionMapper`]: typed-coordinate lookups,
//! within-run character precision, the no-cross-token / no-snap `None` contract, the
//! no-`sourcesContent` run-extent invariant, and the both-space (generated + source)
//! run-compatibility rule that governs range mapping.

use super::*;

/// Construct a generated-TSX query position.
fn ts(line: u32, character: u32) -> TsPosition {
    TsPosition::new(line, character)
}

/// Construct a Vue-source query position.
fn vue(line: u32, character: u32) -> LspPosition {
    LspPosition::new(line, character)
}

/// Helper: build a source map JSON from known token positions.
/// Uses oxc_sourcemap's builder to produce a valid source map.
fn build_test_source_map(
    source_name: &str,
    source_content: &str,
    // Each tuple: (dst_line, dst_col, src_line, src_col)
    tokens: &[(u32, u32, u32, u32)],
) -> String {
    let mut builder = oxc_sourcemap::SourceMapBuilder::default();
    let source_id = builder.set_source_and_content(source_name, source_content);
    for &(dl, dc, sl, sc) in tokens {
        builder.add_token(dl, dc, sl, sc, Some(source_id), None);
    }
    builder.into_sourcemap().to_json_string()
}

/// Helper: build a source map with both mapped and unmapped tokens.
/// Mapped tokens: (dst_line, dst_col, src_line, src_col)
/// Unmapped tokens: (dst_line, dst_col) — emitted with source_id=None
fn build_source_map_with_unmapped(
    source_name: &str,
    source_content: &str,
    mapped: &[(u32, u32, u32, u32)],
    unmapped: &[(u32, u32)],
) -> String {
    let mut builder = oxc_sourcemap::SourceMapBuilder::default();
    let source_id = builder.set_source_and_content(source_name, source_content);

    // Collect all tokens and sort by (line, col)
    let mut all_tokens: Vec<(u32, u32, Option<(u32, u32)>)> = Vec::new();
    for &(dl, dc, sl, sc) in mapped {
        all_tokens.push((dl, dc, Some((sl, sc))));
    }
    for &(dl, dc) in unmapped {
        all_tokens.push((dl, dc, None));
    }
    all_tokens.sort_by_key(|(l, c, _)| (*l, *c));

    for (dl, dc, src) in all_tokens {
        match src {
            Some((sl, sc)) => {
                builder.add_token(dl, dc, sl, sc, Some(source_id), None);
            }
            None => {
                builder.add_token(dl, dc, 0, 0, None, None);
            }
        }
    }

    builder.into_sourcemap().to_json_string()
}

// ========================================================================
// Basic construction
// ========================================================================

#[test]
fn test_from_json_valid_source_map() {
    let json = build_test_source_map("test.vue", "hello", &[(0, 0, 0, 0)]);
    let mapper = PositionMapper::from_json(&json);
    assert!(mapper.is_ok());
}

#[test]
fn test_from_json_invalid_json() {
    let mapper = PositionMapper::from_json("not valid json");
    assert!(mapper.is_err());
}

#[test]
fn test_from_json_empty_mappings() {
    let json = build_test_source_map("test.vue", "", &[]);
    let mapper = PositionMapper::from_json(&json).unwrap();
    assert!(mapper.tsx_to_carrier(ts(0, 0)).is_none());
}

#[test]
fn source_lookup_prefers_the_earliest_generated_run_for_duplicate_origins() {
    // An authored setup declaration is emitted once in the retained script and
    // again as a later template-only ref-unwrapped alias. Both generated names
    // deliberately map to the declaration token. Source requests must select
    // the original script run; the later alias is joined only by
    // references/rename logic that explicitly asks for linked projections.
    let json = build_test_source_map("App.vue", "const count = 0", &[(0, 0, 0, 6), (1, 0, 0, 6)]);
    let mapper = PositionMapper::from_json(&json).unwrap();

    let mapped = mapper
        .carrier_to_tsx(vue(0, 6))
        .expect("the authored declaration has two exact generated projections");
    assert_eq!(mapped.pos, ts(0, 0));
}

// ========================================================================
// tsx_to_carrier (generated -> source)
// ========================================================================

#[test]
fn test_tsx_to_vue_exact_token_match() {
    // gen(0,0) -> src(2,5), gen(1,0) -> src(4,0)
    let json = build_test_source_map(
        "App.vue",
        "<template>\n  <div>hello</div>\n</template>\n\n<script setup>\nconst x = 1;\n</script>",
        &[(0, 0, 2, 5), (1, 0, 4, 0)],
    );
    let mapper = PositionMapper::from_json(&json).unwrap();

    let m = mapper.tsx_to_carrier(ts(0, 0)).unwrap();
    assert_eq!(m.pos, LspPosition::new(2, 5));

    let m = mapper.tsx_to_carrier(ts(1, 0)).unwrap();
    assert_eq!(m.pos, LspPosition::new(4, 0));
}

/// REPLACES the old `test_tsx_to_vue_between_tokens` extrapolation test.
///
/// A query that lands exactly on an unmapped (`Inserted`) token start, immediately
/// after a mapped token, must NOT extrapolate off the preceding token. The old code
/// backward-scanned to the mapped token and added a column delta (`Some`); the strict
/// lookup returns `None` because the covering token is unmapped.
#[test]
fn test_tsx_to_vue_cross_token_no_extrapolation() {
    // mapped gen(0,0)->src(0,0); unmapped gen(0,5) (synthetic prefix begins).
    let json =
        build_source_map_with_unmapped("App.vue", "const x = hello;", &[(0, 0, 0, 0)], &[(0, 5)]);
    let mapper = PositionMapper::from_json(&json).unwrap();

    // Query 5 columns past the mapped token, on the unmapped token -> None.
    assert!(
        mapper.tsx_to_carrier(ts(0, 5)).is_none(),
        "must not extrapolate across an unmapped-token boundary: {:?}",
        mapper.tsx_to_carrier(ts(0, 5))
    );
}

/// Within-run character precision is PRESERVED: a query inside a single mapped
/// multi-character run maps to the corresponding source column. Discriminates the
/// other way — deleting the within-run delta would make this `None`/wrong.
#[test]
fn test_tsx_to_vue_within_run_character_precision() {
    // mapped gen(0,0)->src(0,10) for a 6-char identifier; an unmapped token at
    // gen(0,6) bounds the run to [0,6). Column 3 is interior.
    let json =
        build_source_map_with_unmapped("App.vue", &" ".repeat(40), &[(0, 0, 0, 10)], &[(0, 6)]);
    let mapper = PositionMapper::from_json(&json).unwrap();

    // Column 3 of the 6-char run -> src col 10 + 3 = 13.
    let m = mapper.tsx_to_carrier(ts(0, 3)).unwrap();
    assert_eq!(m.pos, LspPosition::new(0, 13));
    // Run start maps exactly.
    assert_eq!(
        mapper.tsx_to_carrier(ts(0, 0)).unwrap().pos,
        LspPosition::new(0, 10)
    );
    // Last in-run column (5) maps; column 6 is the unmapped boundary -> None.
    assert_eq!(
        mapper.tsx_to_carrier(ts(0, 5)).unwrap().pos,
        LspPosition::new(0, 15)
    );
    assert!(mapper.tsx_to_carrier(ts(0, 6)).is_none());
}

/// Query at the `_ctx.` / `$setup.` prefix columns -> None.
/// Retargets the old `test_prepended_text_inside_prefix_returns_none`.
#[test]
fn test_tsx_to_vue_inside_prefix_returns_none() {
    let json = build_source_map_with_unmapped(
        "App.vue",
        "<template>\n   count\n</template>",
        &[(0, 0, 1, 0), (0, 7, 1, 3)],
        &[(0, 2)], // "_ctx." unmapped at gen col 2
    );
    let mapper = PositionMapper::from_json(&json).unwrap();

    // Inside "_ctx." prefix (gen cols 2-6) -> None.
    assert!(mapper.tsx_to_carrier(ts(0, 3)).is_none());
    assert!(mapper.tsx_to_carrier(ts(0, 5)).is_none());
}

/// Overwritten / synthetic punctuation interior -> None: a query inside an unmapped
/// token that sits BETWEEN two mapped runs returns nothing (no snap to either side).
#[test]
fn test_tsx_to_vue_unmapped_synthetic_interior_returns_none() {
    // mapped gen(0,0)->src(0,0), unmapped gen(0,3) (synthetic), mapped gen(0,8)->src(0,20).
    let json = build_source_map_with_unmapped(
        "App.vue",
        &" ".repeat(40),
        &[(0, 0, 0, 0), (0, 8, 0, 20)],
        &[(0, 3)],
    );
    let mapper = PositionMapper::from_json(&json).unwrap();

    // The mapped run gen[0,3) ends at the unmapped token; col 5 is in the synthetic
    // interior (covered by the unmapped token) -> None.
    assert!(mapper.tsx_to_carrier(ts(0, 5)).is_none());
    // The first mapped run is bounded to [0,3): col 2 still maps within it.
    assert_eq!(
        mapper.tsx_to_carrier(ts(0, 2)).unwrap().pos,
        LspPosition::new(0, 2)
    );
    // The second mapped run starts at gen 8.
    assert_eq!(
        mapper.tsx_to_carrier(ts(0, 8)).unwrap().pos,
        LspPosition::new(0, 20)
    );
}

// ========================================================================
// carrier_to_tsx (source -> generated)
// ========================================================================

#[test]
fn test_vue_to_tsx_exact_token_match() {
    // gen(5,0) -> src(0,0), gen(6,4) -> src(1,2)
    let json = build_test_source_map("App.vue", "hello\n  world", &[(5, 0, 0, 0), (6, 4, 1, 2)]);
    let mapper = PositionMapper::from_json(&json).unwrap();

    assert_eq!(
        mapper.carrier_to_tsx(vue(0, 0)).unwrap().pos,
        TsPosition::new(5, 0)
    );
    assert_eq!(
        mapper.carrier_to_tsx(vue(1, 2)).unwrap().pos,
        TsPosition::new(6, 4)
    );
}

/// REPLACES the old `test_vue_to_tsx_offset_within_token_range` snap test.
///
/// A source position on a line with NO mapping must return `None`. The old code
/// snapped to the closest preceding token (even on an earlier line) and returned
/// `Some`; the strict lookup requires an in-run token on the SAME source line.
#[test]
fn test_vue_to_tsx_unmapped_source_returns_none() {
    // Only token maps src(0,0)->gen(0,0). Lines 1..=5 have no mapping.
    let json = build_test_source_map("App.vue", "a\n\n\n\n\nb", &[(0, 0, 0, 0)]);
    let mapper = PositionMapper::from_json(&json).unwrap();

    // src(5,0): a Vue line with no mapped token -> None (no snap to line 0).
    assert!(
        mapper.carrier_to_tsx(vue(5, 0)).is_none(),
        "unmapped source line must not snap to a preceding token: {:?}",
        mapper.carrier_to_tsx(vue(5, 0))
    );
    // src(0,0) still maps exactly.
    assert_eq!(
        mapper.carrier_to_tsx(vue(0, 0)).unwrap().pos,
        TsPosition::new(0, 0)
    );
}

/// Within-run character precision for `carrier_to_tsx`: a target inside a single mapped
/// source run maps to the corresponding generated column.
#[test]
fn test_vue_to_tsx_within_run_character_precision() {
    // Token tuples are (dst_line, dst_col, src_line, src_col):
    //   gen(0,10) -> src(0,7)  — start of the source run
    //   gen(0,40) -> src(0,20) — next mapped token, bounds the run to src [7,20)
    let json = build_test_source_map("App.vue", &" ".repeat(40), &[(0, 10, 0, 7), (0, 40, 0, 20)]);
    let mapper = PositionMapper::from_json(&json).unwrap();

    // src col 13 -> gen col 10 + (13-7) = 16.
    assert_eq!(
        mapper.carrier_to_tsx(vue(0, 13)).unwrap().pos,
        TsPosition::new(0, 16)
    );
    // Run start: src 7 -> gen 10.
    assert_eq!(
        mapper.carrier_to_tsx(vue(0, 7)).unwrap().pos,
        TsPosition::new(0, 10)
    );
    // Last in-run col (19) -> gen 22; col 20 enters the next run (maps via that token).
    assert_eq!(
        mapper.carrier_to_tsx(vue(0, 19)).unwrap().pos,
        TsPosition::new(0, 22)
    );
    assert_eq!(
        mapper.carrier_to_tsx(vue(0, 20)).unwrap().pos,
        TsPosition::new(0, 40)
    );
}

#[test]
fn test_vue_to_tsx_no_tokens() {
    let json = build_test_source_map("App.vue", "", &[]);
    let mapper = PositionMapper::from_json(&json).unwrap();
    assert!(mapper.carrier_to_tsx(vue(0, 0)).is_none());
}

#[test]
fn test_vue_to_tsx_position_before_all_tokens() {
    // All tokens start at src(2,0) or later; query before them -> None.
    let json = build_test_source_map("App.vue", "\n\nhello", &[(5, 0, 2, 0)]);
    let mapper = PositionMapper::from_json(&json).unwrap();
    assert!(mapper.carrier_to_tsx(vue(0, 0)).is_none());
}

#[test]
fn test_vue_to_tsx_multiline_source() {
    // Script block at Vue lines 5/6 mapped to TSX lines 0/1. The last-on-line run
    // extends to the SOURCE line's true content length (position-preserving), so the
    // queried columns must lie within the real `const x = 1;` / `const y = 2;` text.
    let json = build_test_source_map(
        "App.vue",
        "<template>\n  <div/>\n</template>\n\n<script setup>\nconst x = 1;\nconst y = 2;\n</script>",
        &[(0, 0, 5, 0), (1, 0, 6, 0)],
    );
    let mapper = PositionMapper::from_json(&json).unwrap();

    // src(5,6) -> gen(0,6): "x" in "const x = 1;" (col 6 < line length 12).
    assert_eq!(
        mapper.carrier_to_tsx(vue(5, 6)).unwrap().pos,
        TsPosition::new(0, 6)
    );
    // src(6,6) -> gen(1,6).
    assert_eq!(
        mapper.carrier_to_tsx(vue(6, 6)).unwrap().pos,
        TsPosition::new(1, 6)
    );
}

// ========================================================================
// Within-run delta bounded by the token's TRUE extent (not EOL, not "to the
// next MAPPED token"). A query in a GAP between two mapped tokens, or past the
// real content end of the last token on a line, returns None.
// ========================================================================

/// `carrier_to_tsx`: a source query in a GAP between two mapped tokens — where the first
/// token's TRUE content (bounded by the next dst token of ANY kind) ends before the
/// gap — must be `None`, NOT snapped into the preceding run.
///
/// Discriminating: a source run bounded by the next *mapped* source token would span the
/// whole gap [0,20), so a query at src col 12 would snap into it (mapping to `gen 0+12`).
/// With true-extent bounding the first run is only [0,3) (an unmapped dst token at gen
/// col 3 bounds it), so src col 12 is in no run -> `None`.
#[test]
fn test_vue_to_tsx_gap_between_tokens_returns_none() {
    // Position-preserving: dst col == src col here.
    //   mapped gen(0,0)->src(0,0)   — run bounded by the unmapped dst token at gen 3
    //   unmapped gen(0,3)           — synthetic content begins
    //   mapped gen(0,20)->src(0,20) — the next mapped run
    let json = build_source_map_with_unmapped(
        "App.vue",
        &" ".repeat(40),
        &[(0, 0, 0, 0), (0, 20, 0, 20)],
        &[(0, 3)],
    );
    let mapper = PositionMapper::from_json(&json).unwrap();

    // src col 12 is in the gap between the first run [0,3) and the second run [20,..).
    // It belongs to NO mapped run -> None (no snap into the first run).
    assert!(
        mapper.carrier_to_tsx(vue(0, 12)).is_none(),
        "source query in an inter-token gap must not snap to the preceding run: {:?}",
        mapper.carrier_to_tsx(vue(0, 12))
    );
    // The first run's true interior (col 2, inside [0,3)) still maps.
    assert_eq!(
        mapper.carrier_to_tsx(vue(0, 2)).unwrap().pos,
        TsPosition::new(0, 2)
    );
    // The last in-run col of the first run (2) maps; col 3 is the boundary -> None.
    assert!(mapper.carrier_to_tsx(vue(0, 3)).is_none());
    // The second run still maps at its start.
    assert_eq!(
        mapper.carrier_to_tsx(vue(0, 20)).unwrap().pos,
        TsPosition::new(0, 20)
    );
}

/// `tsx_to_carrier`: a query past the real content end of the LAST mapped token on a line
/// must be `None`. The run's extent is the SOURCE line's true content length, not EOL.
///
/// Discriminating: a "last run extends to end-of-line" strategy would map a query at gen
/// col 5 on a 2-char source line to `Some(src 0+5)`. With true content-length bounding the
/// run is [0,2), so col 5 -> `None`.
#[test]
fn test_tsx_to_vue_past_last_token_content_returns_none() {
    // Source line 0 is "ab" (length 2). One mapped token gen(0,0)->src(0,0), no
    // following dst token of any kind -> run length = source-line-len(2) - 0 = 2.
    let json = build_test_source_map("App.vue", "ab", &[(0, 0, 0, 0)]);
    let mapper = PositionMapper::from_json(&json).unwrap();

    // Within the 2-char run: cols 0 and 1 map.
    assert_eq!(
        mapper.tsx_to_carrier(ts(0, 0)).unwrap().pos,
        LspPosition::new(0, 0)
    );
    assert_eq!(
        mapper.tsx_to_carrier(ts(0, 1)).unwrap().pos,
        LspPosition::new(0, 1)
    );
    // Col 2 is one-past the real content end -> None (not extended to EOL).
    assert!(
        mapper.tsx_to_carrier(ts(0, 2)).is_none(),
        "query at the content end of the last run must be None: {:?}",
        mapper.tsx_to_carrier(ts(0, 2))
    );
    // Col 5 (well past content) -> None.
    assert!(mapper.tsx_to_carrier(ts(0, 5)).is_none());
}

/// Symmetric: `carrier_to_tsx` past the real content end of the last mapped token on a
/// source line must be `None` (true source-line-length bound, not EOL).
#[test]
fn test_vue_to_tsx_past_last_token_content_returns_none() {
    // Source line 0 is "ab" (length 2). gen(0,5)->src(0,0); last on both lines.
    let json = build_test_source_map("App.vue", "ab", &[(0, 5, 0, 0)]);
    let mapper = PositionMapper::from_json(&json).unwrap();

    assert_eq!(
        mapper.carrier_to_tsx(vue(0, 1)).unwrap().pos,
        TsPosition::new(0, 6)
    );
    assert!(
        mapper.carrier_to_tsx(vue(0, 2)).is_none(),
        "source query past content end must be None: {:?}",
        mapper.carrier_to_tsx(vue(0, 2))
    );
}

/// Over-correction guard: a legitimate multiline mapped expression — where the queried
/// columns ARE within the real source content of each line — still maps on every line.
#[test]
fn test_multiline_mapped_expression_in_bounds_still_maps() {
    // Source lines 4, 5, 6 each hold real 12-char content "const v = 1;"; gen line i
    // maps to src line 4+i. (Lines 0-3 are filler so the indices match a realistic SFC.)
    let src = "\n\n\n\nconst v = 1;\nconst v = 1;\nconst v = 1;\n";
    let json = build_test_source_map("App.vue", src, &[(0, 0, 4, 0), (1, 0, 5, 0), (2, 0, 6, 0)]);
    let mapper = PositionMapper::from_json(&json).unwrap();

    // Cols 2/3/4 are all within the 12-char source lines.
    assert_eq!(
        mapper.tsx_to_carrier(ts(0, 2)).unwrap().pos,
        LspPosition::new(4, 2)
    );
    assert_eq!(
        mapper.tsx_to_carrier(ts(1, 3)).unwrap().pos,
        LspPosition::new(5, 3)
    );
    assert_eq!(
        mapper.tsx_to_carrier(ts(2, 4)).unwrap().pos,
        LspPosition::new(6, 4)
    );

    // Roundtrip on the middle line.
    let tsx = mapper.carrier_to_tsx(vue(5, 3)).unwrap().pos;
    assert_eq!(tsx, TsPosition::new(1, 3));
    assert_eq!(
        mapper
            .tsx_to_carrier(ts(tsx.line, tsx.character))
            .unwrap()
            .pos,
        LspPosition::new(5, 3)
    );
}

// ========================================================================
// Range endpoint compatibility (the strict range API). A range maps only
// when both endpoints resolve inside COMPATIBLE runs (same run, or
// genuinely-contiguous mapped runs with no synthetic content between them).
// Otherwise the range is DROPPED (None), never a bogus range.
// ========================================================================

/// A range whose two endpoints fall in two DIFFERENT mapped runs separated by
/// synthetic/unmapped content -> the range is DROPPED (None).
///
/// Discriminating: a per-endpoint composer maps each endpoint independently; both
/// endpoints DO map (start in run A, end in run B), so it returns a (bogus) Vue range
/// straddling the synthetic content. The strict range API rejects incompatible runs.
#[test]
fn test_tsx_range_cross_run_with_synthetic_between_returns_none() {
    // mapped gen(0,0)->src(0,0) run [0,3); unmapped gen(0,3) (synthetic);
    // mapped gen(0,8)->src(0,50) run [8,..). Start in run A, end in run B.
    let json = build_source_map_with_unmapped(
        "App.vue",
        &" ".repeat(80),
        &[(0, 0, 0, 0), (0, 8, 0, 50)],
        &[(0, 3)],
    );
    let mapper = PositionMapper::from_json(&json).unwrap();

    // Both endpoints individually map (start col 1 -> run A, end col 9 -> run B),
    // but the runs are separated by synthetic content at gen col 3 -> None.
    assert!(
        mapper.tsx_to_carrier(ts(0, 1)).is_some(),
        "precondition: start endpoint maps"
    );
    assert!(
        mapper.tsx_to_carrier(ts(0, 9)).is_some(),
        "precondition: end endpoint maps"
    );
    assert!(
        mapper.tsx_range_to_carrier(ts(0, 1), ts(0, 9)).is_none(),
        "a range straddling synthetic content between two runs must be dropped: {:?}",
        mapper.tsx_range_to_carrier(ts(0, 1), ts(0, 9))
    );
}

/// A range fully inside ONE mapped run maps correctly (start + half-open end).
#[test]
fn test_tsx_range_within_single_run_maps() {
    // One mapped run gen(0,0)->src(0,10), bounded to [0,6) by an unmapped token at 6.
    let json =
        build_source_map_with_unmapped("App.vue", &" ".repeat(40), &[(0, 0, 0, 10)], &[(0, 6)]);
    let mapper = PositionMapper::from_json(&json).unwrap();

    // Range [1,4) inside the run -> Vue [11,14).
    let (start, end) = mapper.tsx_range_to_carrier(ts(0, 1), ts(0, 4)).unwrap();
    assert_eq!(start, LspPosition::new(0, 11));
    assert_eq!(end, LspPosition::new(0, 14));

    // Half-open end exactly at the run's exclusive end (gen col 6 == run end) maps to
    // the run's mapped source end (src col 16).
    let (start, end) = mapper.tsx_range_to_carrier(ts(0, 0), ts(0, 6)).unwrap();
    assert_eq!(start, LspPosition::new(0, 10));
    assert_eq!(end, LspPosition::new(0, 16));
}

/// A range across two GENUINELY-CONTIGUOUS mapped runs (no synthetic content between:
/// run A's dst end == run B's dst start) maps. The endpoints are compatible.
#[test]
fn test_tsx_range_contiguous_runs_maps() {
    // Two adjacent mapped runs on gen line 0, mapping to the SAME contiguous source:
    //   gen(0,0)->src(0,0) run [0,3)
    //   gen(0,3)->src(0,3) run [3,6)  (contiguous: A.dst_end 3 == B.dst_col 3)
    //   unmapped gen(0,6) bounds run B.
    let json = build_source_map_with_unmapped(
        "App.vue",
        &" ".repeat(40),
        &[(0, 0, 0, 0), (0, 3, 0, 3)],
        &[(0, 6)],
    );
    let mapper = PositionMapper::from_json(&json).unwrap();

    // Range [1,5): start in run A, end in run B; runs are contiguous -> maps.
    let (start, end) = mapper.tsx_range_to_carrier(ts(0, 1), ts(0, 5)).unwrap();
    assert_eq!(start, LspPosition::new(0, 1));
    assert_eq!(end, LspPosition::new(0, 5));
}

/// Two runs that are GENERATED-adjacent but SOURCE-discontiguous must NOT compose into a
/// range. The generated output relocates/repeats source (`MoveOriginal`, v-model emitting
/// the same expression several times), so dst-adjacent runs routinely map to reordered /
/// repeated / non-adjacent source. Generated adjacency is necessary but NOT sufficient for
/// a well-formed SOURCE range; the source side must be contiguous too.
///
/// Discriminating: a dst-only compatibility rule (`left.dst_end == right.dst_col`) treats
/// these as compatible and composes the bogus Vue range `[0..50)` spanning unrelated
/// source. The source-contiguity component rule rejects them -> `None`.
#[test]
fn test_tsx_range_dst_adjacent_src_discontiguous_returns_none() {
    // run A gen(0,0)->src(0,0) [0,3); run B gen(0,3)->src(0,50) [3,..) bounded by an
    // unmapped token at gen 6. A.dst_end (3) == B.dst_col (3) -> generated-adjacent.
    // A.src_end (3) != B.src_col (50) -> source-DIScontiguous.
    let json = build_source_map_with_unmapped(
        "App.vue",
        &" ".repeat(80),
        &[(0, 0, 0, 0), (0, 3, 0, 50)],
        &[(0, 6)],
    );
    let mapper = PositionMapper::from_json(&json).unwrap();

    // Each endpoint maps individually (start col 1 -> run A, end col 4 -> run B).
    assert!(
        mapper.tsx_to_carrier(ts(0, 1)).is_some(),
        "precondition: start endpoint maps in run A"
    );
    assert!(
        mapper.tsx_to_carrier(ts(0, 4)).is_some(),
        "precondition: end endpoint maps in run B"
    );
    // The runs are dst-adjacent but src-discontiguous -> the range must be dropped.
    assert!(
        mapper.tsx_range_to_carrier(ts(0, 1), ts(0, 4)).is_none(),
        "dst-adjacent + src-discontiguous runs must not compose a range: {:?}",
        mapper.tsx_range_to_carrier(ts(0, 1), ts(0, 4))
    );
}

/// A multiline mapped expression whose runs wrap across lines in BOTH spaces (gen line N
/// EOL -> gen line N+1 col 0, and likewise src line M EOL -> src line M+1 col 0) is a
/// legitimate single compatibility component: such a range must still compose. This guards
/// the source-contiguity rule against over-correcting away genuine multiline expressions.
#[test]
fn test_tsx_range_multiline_wrap_runs_compose() {
    // Source lines 0 and 1 each hold real 5-char content "abcde"; gen line 0 maps to src
    // line 0 and gen line 1 maps to src line 1 (a 2-line mapped expression).
    //   run A gen(0,0)->src(0,0): bounded by src-line-len 5 -> [0,5) on both sides, ending
    //         at the EOL of gen line 0 / src line 0.
    //   run B gen(1,0)->src(1,0): starts at col 0 of the next line on both sides.
    let json = build_test_source_map("App.vue", "abcde\nabcde", &[(0, 0, 0, 0), (1, 0, 1, 0)]);
    let mapper = PositionMapper::from_json(&json).unwrap();

    // Range from inside run A on line 0 to inside run B on line 1: the line-wrap
    // transition is contiguous in both spaces -> the range composes.
    let (start, end) = mapper
        .tsx_range_to_carrier(ts(0, 2), ts(1, 3))
        .expect("a genuine multiline-wrap expression range must compose");
    assert_eq!(start, LspPosition::new(0, 2));
    assert_eq!(end, LspPosition::new(1, 3));
}

// ========================================================================
// No-`sourcesContent` run-extent path. When the source map carries no
// embedded source content, an INTERIOR run's source extent is bounded by the next
// generated token via the position-preserving invariant (`src_end - src_col ==
// dst_end - dst_col`) — so it is provably the TRUE extent, NOT a silently permissive
// fallback, and is observably IDENTICAL to the with-content case for the same interior
// geometry. A last-on-line run with no content has no extent signal at all and is
// conservatively DROPPED (maps nothing) — also non-permissive.
// ========================================================================

/// Build a source map JSON with NO embedded `sourcesContent` (the `sources` array is
/// present but `sourcesContent` is absent), exercising the `(next_dst_bound, None)` arm of
/// `precompute_runs`. Reuses the proven token encoder, then strips the content field.
fn build_source_map_without_content(
    source_name: &str,
    // Each tuple: (dst_line, dst_col, src_line, src_col)
    tokens: &[(u32, u32, u32, u32)],
) -> String {
    // The builder requires content to encode tokens; build with placeholder content, then
    // remove the `sourcesContent` key so the decoded map has empty source contents.
    let with_content = build_test_source_map(source_name, &" ".repeat(80), tokens);
    let mut value: serde_json::Value = serde_json::from_str(&with_content).unwrap();
    value
        .as_object_mut()
        .unwrap()
        .remove("sourcesContent")
        .expect("builder JSON should carry sourcesContent to strip");
    serde_json::to_string(&value).unwrap()
}

/// Sanity: a no-content map really does decode with NO source contents (so the
/// `(Some(a), None)` run-extent arm is the one under test, not the with-content arm).
#[test]
fn test_no_sources_content_decodes_without_contents() {
    let json = build_source_map_without_content("App.vue", &[(0, 0, 0, 0)]);
    let mapper = PositionMapper::from_json(&json).unwrap();
    assert!(
        mapper
            .source_map()
            .get_source_contents()
            .all(|c| c.is_none()),
        "no-sourcesContent map must decode with empty/None source contents"
    );
}

/// No-`sourcesContent` INTERIOR run is bounded by the next generated token via the
/// position-preserving invariant — it is NOT silently permissive. The first run is bounded
/// by the second token at gen col 3, so it is `[0,3)` on BOTH sides: a query past col 3 is
/// out of the first run.
///
/// Discriminating: a permissive "no source-line info -> extend to EOL / unbounded" rule
/// would let the first run extend to col 6 and map a query at col 5 to `Some(src 0+5)`.
/// The invariant-bounded interior run rejects col 5 (it is the SECOND run's interior).
#[test]
fn test_no_sources_content_interior_run_bounded_by_next_token_not_permissive() {
    // No `sourcesContent`. Three tokens so the FIRST run is interior (bounded by token 2)
    // and the SECOND run is interior (bounded by token 3):
    //   gen(0,0)->src(0,0) bounded by gen 3 -> run [0,3)
    //   gen(0,3)->src(0,3) bounded by gen 6 -> run [3,6)
    //   gen(0,6)->src(0,6) (last-on-line, no content) -> dropped
    let json =
        build_source_map_without_content("App.vue", &[(0, 0, 0, 0), (0, 3, 0, 3), (0, 6, 0, 6)]);
    let mapper = PositionMapper::from_json(&json).unwrap();

    // First run interior: col 2 maps within [0,3).
    assert_eq!(
        mapper.tsx_to_carrier(ts(0, 2)).unwrap().pos,
        LspPosition::new(0, 2)
    );
    // Col 5 is NOT in the first run [0,3); it is the second run [3,6)'s interior -> src 5.
    // A permissive first run (extend-to-EOL) would have instead mapped col 5 off the FIRST
    // run; the invariant bound forbids that. Either way col 5 must map via the SECOND run.
    assert_eq!(
        mapper.tsx_to_carrier(ts(0, 5)).unwrap().pos,
        LspPosition::new(0, 5)
    );
    // The last-on-line token (gen 6) has no extent signal without content -> dropped, so
    // col 6 maps to nothing (non-permissive: no extend-to-EOL).
    assert!(
        mapper.tsx_to_carrier(ts(0, 6)).is_none(),
        "a last-on-line no-content run has no extent signal and must be dropped, not \
         extended: {:?}",
        mapper.tsx_to_carrier(ts(0, 6))
    );
}

/// The no-`sourcesContent` path produces the SAME run geometry as the with-content path
/// for an INTERIOR run (the position-preserving invariant makes the source extent equal
/// the generated extent, which `next_dst_bound` supplies either way). This is the
/// definitive non-permissive proof: interior behaviour does not differ unobservably based
/// on whether content is present.
#[test]
fn test_no_sources_content_interior_matches_with_content_geometry() {
    // Two interior runs (each bounded by a following token) + a last token. The probed
    // columns 0..6 all fall inside the two INTERIOR runs, where the invariant guarantees
    // identical geometry with or without content.
    let tokens = &[(0u32, 0u32, 0u32, 0u32), (0, 3, 0, 3), (0, 6, 0, 6)];
    let with =
        PositionMapper::from_json(&build_test_source_map("App.vue", &" ".repeat(80), tokens))
            .unwrap();
    let without =
        PositionMapper::from_json(&build_source_map_without_content("App.vue", tokens)).unwrap();

    // Interior columns map identically (Some/None and value) in both maps.
    for col in 0..6u32 {
        assert_eq!(
            with.tsx_to_carrier(ts(0, col)).map(|m| m.pos),
            without.tsx_to_carrier(ts(0, col)).map(|m| m.pos),
            "interior tsx_to_carrier diverges with vs without sourcesContent at col {col}"
        );
    }
}

// ========================================================================
// Roundtrips
// ========================================================================

#[test]
fn test_roundtrip_exact_token_positions() {
    // 1:1 identity mapping (common for script blocks).
    let json = build_test_source_map(
        "App.vue",
        "const x = 1;\nconst y = 2;",
        &[(0, 0, 0, 0), (1, 0, 1, 0)],
    );
    let mapper = PositionMapper::from_json(&json).unwrap();

    let tsx = mapper.carrier_to_tsx(vue(0, 0)).unwrap().pos;
    let back = mapper
        .tsx_to_carrier(ts(tsx.line, tsx.character))
        .unwrap()
        .pos;
    assert_eq!(back, LspPosition::new(0, 0));

    let tsx = mapper.carrier_to_tsx(vue(1, 0)).unwrap().pos;
    let back = mapper
        .tsx_to_carrier(ts(tsx.line, tsx.character))
        .unwrap()
        .pos;
    assert_eq!(back, LspPosition::new(1, 0));
}

/// Exactly-mapped identifier: source -> generated -> source == original, for EVERY
/// character of the run (prefix-shifted mapping, the `$setup.count` shape).
#[test]
fn test_roundtrip_exact_mapped_text_identity() {
    // gen "$setup.count": "$setup." unmapped at gen 0, "count" mapped gen(0,7)->src(3,14).
    let json = build_source_map_with_unmapped(
        "App.vue",
        "<script setup>\nconst count = 0\n</script>\n<template>{{ count }}</template>",
        &[(0, 7, 3, 14)],
        &[(0, 0)],
    );
    let mapper = PositionMapper::from_json(&json).unwrap();

    for i in 0..5u32 {
        let vue_col = 14 + i;
        let tsx = mapper.carrier_to_tsx(vue(3, vue_col)).unwrap().pos;
        assert_eq!(tsx, TsPosition::new(0, 7 + i), "forward char {i}");

        let back = mapper
            .tsx_to_carrier(ts(tsx.line, tsx.character))
            .unwrap()
            .pos;
        assert_eq!(back, LspPosition::new(3, vue_col), "roundtrip char {i}");
    }
}

/// Half-open boundary: one-past-end of a mapped run resolves only when that endpoint
/// is itself inside a compatible mapped run; otherwise `None`.
#[test]
fn test_half_open_one_past_end() {
    // Two adjacent mapped runs on gen line 0: [0,3) src(0,0); [3,6) src(0,20); then
    // an unmapped token at gen 6 bounds the second run.
    let json = build_source_map_with_unmapped(
        "App.vue",
        &" ".repeat(40),
        &[(0, 0, 0, 0), (0, 3, 0, 20)],
        &[(0, 6)],
    );
    let mapper = PositionMapper::from_json(&json).unwrap();

    // One past the first run's start cluster: gen col 3 is the START of the second
    // run, so it resolves into the SECOND run (src 20), not an extrapolation of the
    // first.
    assert_eq!(
        mapper.tsx_to_carrier(ts(0, 3)).unwrap().pos,
        LspPosition::new(0, 20)
    );
    // One past the end of the (bounded) second run: gen col 6 is the unmapped
    // boundary -> None.
    assert!(mapper.tsx_to_carrier(ts(0, 6)).is_none());
    // Interior of the second run still maps.
    assert_eq!(
        mapper.tsx_to_carrier(ts(0, 5)).unwrap().pos,
        LspPosition::new(0, 22)
    );
}

// ========================================================================
// Prepended-text (e.g. _ctx. / $setup.) forward + reverse
// ========================================================================

#[test]
fn test_prepended_text_forward_mapping_start_middle_end() {
    let json = build_source_map_with_unmapped(
        "App.vue",
        "<template>\n   count\n</template>",
        &[
            (0, 0, 1, 0), // spaces before count
            (0, 7, 1, 3), // "count" starts (after "_ctx.")
        ],
        &[(0, 2)], // "_ctx." inserted at gen col 2
    );
    let mapper = PositionMapper::from_json(&json).unwrap();

    // start of "count": Vue(1,3) -> TSX(0,7)
    assert_eq!(
        mapper.carrier_to_tsx(vue(1, 3)).unwrap().pos,
        TsPosition::new(0, 7)
    );
    // middle ('u', col 5) -> TSX col 9
    assert_eq!(
        mapper.carrier_to_tsx(vue(1, 5)).unwrap().pos,
        TsPosition::new(0, 9)
    );
    // end ('t', col 7) -> TSX col 11
    assert_eq!(
        mapper.carrier_to_tsx(vue(1, 7)).unwrap().pos,
        TsPosition::new(0, 11)
    );
}

#[test]
fn test_prepended_text_reverse_mapping_start_middle_end() {
    let json = build_source_map_with_unmapped(
        "App.vue",
        "<template>\n   count\n</template>",
        &[(0, 0, 1, 0), (0, 7, 1, 3)],
        &[(0, 2)],
    );
    let mapper = PositionMapper::from_json(&json).unwrap();

    // TSX col 7 ('c') -> Vue(1,3)
    assert_eq!(
        mapper.tsx_to_carrier(ts(0, 7)).unwrap().pos,
        LspPosition::new(1, 3)
    );
    // TSX col 9 ('u') -> Vue(1,5)
    assert_eq!(
        mapper.tsx_to_carrier(ts(0, 9)).unwrap().pos,
        LspPosition::new(1, 5)
    );
    // TSX col 11 ('t') -> Vue(1,7)
    assert_eq!(
        mapper.tsx_to_carrier(ts(0, 11)).unwrap().pos,
        LspPosition::new(1, 7)
    );
}

#[test]
fn test_prepended_text_multiple_bindings_on_same_line() {
    // "{{ a }} {{ b }}" -> "$setup.a $setup.b"
    let json = build_source_map_with_unmapped(
        "App.vue",
        "<template>\n<div>{{ a }} {{ b }}</div>\n</template>",
        &[
            (0, 7, 1, 9),   // "a"
            (0, 9, 1, 11),  // " " between interpolations
            (0, 16, 1, 18), // "b"
        ],
        &[
            (0, 0), // "$setup." for a
            (0, 8), // separator (end of a region)
            (0, 9), // "$setup." for b
        ],
    );
    let mapper = PositionMapper::from_json(&json).unwrap();

    assert_eq!(
        mapper.carrier_to_tsx(vue(1, 9)).unwrap().pos,
        TsPosition::new(0, 7)
    );
    assert_eq!(
        mapper.carrier_to_tsx(vue(1, 18)).unwrap().pos,
        TsPosition::new(0, 16)
    );
    assert_eq!(
        mapper.tsx_to_carrier(ts(0, 7)).unwrap().pos,
        LspPosition::new(1, 9)
    );
    assert_eq!(
        mapper.tsx_to_carrier(ts(0, 16)).unwrap().pos,
        LspPosition::new(1, 18)
    );
}

// ========================================================================
// CRLF / tabs / multiline mapped expression
// ========================================================================

/// CRLF line endings must not perturb the line/column mapping (positions are
/// line/character pairs, so the `\r` is irrelevant as long as the run is in-run).
#[test]
fn test_crlf_mapping() {
    // Source uses CRLF; script identifier "value" at src(1,6) maps to gen(0,6).
    let json = build_test_source_map(
        "App.vue",
        "<script setup>\r\nconst value = 1;\r\n</script>\r\n",
        &[(0, 6, 1, 6), (0, 40, 1, 40)],
    );
    let mapper = PositionMapper::from_json(&json).unwrap();

    // Forward + within-run + roundtrip across the CRLF-terminated line.
    assert_eq!(
        mapper.carrier_to_tsx(vue(1, 6)).unwrap().pos,
        TsPosition::new(0, 6)
    );
    assert_eq!(
        mapper.carrier_to_tsx(vue(1, 8)).unwrap().pos,
        TsPosition::new(0, 8)
    );
    let back = mapper.tsx_to_carrier(ts(0, 8)).unwrap().pos;
    assert_eq!(back, LspPosition::new(1, 8));
}

/// Tab-indented source: columns are UTF-16 code units (a tab is one unit), so a
/// run after tab indentation still maps by column arithmetic.
#[test]
fn test_tabs_mapping() {
    // "\t\tconst x" — two tabs (cols 0,1), identifier run mapped at src(0,8)->gen(0,2).
    let json = build_test_source_map(
        "App.vue",
        "\t\tconst x = 1;",
        &[(0, 2, 0, 8), (0, 20, 0, 30)],
    );
    let mapper = PositionMapper::from_json(&json).unwrap();

    assert_eq!(
        mapper.carrier_to_tsx(vue(0, 8)).unwrap().pos,
        TsPosition::new(0, 2)
    );
    // within-run interior after the tabs
    assert_eq!(
        mapper.carrier_to_tsx(vue(0, 10)).unwrap().pos,
        TsPosition::new(0, 4)
    );
    let back = mapper.tsx_to_carrier(ts(0, 4)).unwrap().pos;
    assert_eq!(back, LspPosition::new(0, 10));
}

// NOTE: the legitimate multiline-mapped-expression case is covered by
// `test_multiline_mapped_expression_in_bounds_still_maps`, which models realistic source
// line content so the queried columns lie within the true source text. An earlier variant
// mapped columns 2-4 past a 1-char-per-line source, which is exactly the over-permissive
// "extend last run to EOL" behaviour the true-extent run bounding removes.

// ========================================================================
// UTF-16 / surrogate / astral / non-ASCII (columns are UTF-16 code units)
// ========================================================================

/// Emoji (😀 = 2 UTF-16 units) before a binding; the mapped "msg" run carries the
/// within-run delta in UTF-16 columns.
#[test]
fn test_utf16_emoji_before_binding_forward() {
    let json = build_source_map_with_unmapped(
        "App.vue",
        "😀{{ msg }}",
        &[(0, 7, 0, 5)], // "msg" start (UTF-16 col 5) -> gen col 7
        &[(0, 0)],       // "$setup." prefix unmapped
    );
    let mapper = PositionMapper::from_json(&json).unwrap();

    assert_eq!(
        mapper.carrier_to_tsx(vue(0, 5)).unwrap().pos,
        TsPosition::new(0, 7)
    );
    assert_eq!(
        mapper.carrier_to_tsx(vue(0, 6)).unwrap().pos,
        TsPosition::new(0, 8)
    );
    assert_eq!(
        mapper.carrier_to_tsx(vue(0, 7)).unwrap().pos,
        TsPosition::new(0, 9)
    );
}

#[test]
fn test_utf16_emoji_before_binding_reverse() {
    let json = build_source_map_with_unmapped("App.vue", "😀{{ msg }}", &[(0, 7, 0, 5)], &[(0, 0)]);
    let mapper = PositionMapper::from_json(&json).unwrap();

    assert_eq!(
        mapper.tsx_to_carrier(ts(0, 7)).unwrap().pos,
        LspPosition::new(0, 5)
    );
    assert_eq!(
        mapper.tsx_to_carrier(ts(0, 8)).unwrap().pos,
        LspPosition::new(0, 6)
    );
    assert_eq!(
        mapper.tsx_to_carrier(ts(0, 9)).unwrap().pos,
        LspPosition::new(0, 7)
    );
}

/// Non-ASCII identifier (café — 'é' is 1 UTF-16 unit) roundtrips per character.
#[test]
fn test_utf16_non_ascii_identifier_roundtrip() {
    let json = build_source_map_with_unmapped(
        "App.vue",
        "{{ café }}",
        &[(0, 5, 0, 3)], // "café" starts
        &[(0, 0)],       // "_ctx." prefix
    );
    let mapper = PositionMapper::from_json(&json).unwrap();

    for i in 0..4u32 {
        let vue_col = 3 + i;
        let tsx = mapper.carrier_to_tsx(vue(0, vue_col)).unwrap().pos;
        assert_eq!(tsx.character, 5 + i, "café[{i}] forward");
        let back = mapper
            .tsx_to_carrier(ts(tsx.line, tsx.character))
            .unwrap()
            .pos;
        assert_eq!(back.character, vue_col, "café[{i}] roundtrip");
    }
}

/// Surrogate pair (😀 = 2 UTF-16 units) inside an identifier "a😀b".
#[test]
fn test_utf16_surrogate_pair_in_identifier() {
    let json = build_source_map_with_unmapped(
        "App.vue",
        "{{ a😀b }}",
        &[(0, 5, 0, 3)], // "a😀b" starts
        &[(0, 0)],       // "_ctx." prefix
    );
    let mapper = PositionMapper::from_json(&json).unwrap();

    // 'a' col 3 -> gen 5
    assert_eq!(mapper.carrier_to_tsx(vue(0, 3)).unwrap().pos.character, 5);
    assert_eq!(mapper.tsx_to_carrier(ts(0, 5)).unwrap().pos.character, 3);
    // 😀 first unit col 4 -> gen 6
    assert_eq!(mapper.carrier_to_tsx(vue(0, 4)).unwrap().pos.character, 6);
    assert_eq!(mapper.tsx_to_carrier(ts(0, 6)).unwrap().pos.character, 4);
    // 😀 second unit col 5 -> gen 7
    assert_eq!(mapper.carrier_to_tsx(vue(0, 5)).unwrap().pos.character, 7);
    assert_eq!(mapper.tsx_to_carrier(ts(0, 7)).unwrap().pos.character, 5);
    // 'b' col 6 -> gen 8
    assert_eq!(mapper.carrier_to_tsx(vue(0, 6)).unwrap().pos.character, 8);
    assert_eq!(mapper.tsx_to_carrier(ts(0, 8)).unwrap().pos.character, 6);
}

// ========================================================================
// Line-wrap contiguity must not bridge a synthetic/unmapped tail. A run that
// is NOT the last token on its generated line (a synthetic tail follows it),
// or whose source extent does not reach its source line's true end, must NOT
// join the next-line run into one compatibility component — otherwise a range
// from it into the next-line run composes an over-broad source span across the
// synthetic tail.
// ========================================================================

/// A mapped run with a synthetic/unmapped TAIL on generated line N, followed by a mapped
/// run at line N+1 col 0 (the source wrapping line+1 col 0 too), must land in DIFFERENT
/// compatibility components — so a range across them is dropped.
///
/// Discriminating: a geometry-only `line_wrap` rule (`cur.dst_line==prev.dst_line+1 &&
/// cur.dst_col==0 && cur.src_line==prev.src_line+1 && cur.src_col==0`) joins them because the
/// unmapped tail produces no run, so `runs.last()` is still `prev` when `cur` is processed —
/// and a range from `prev` into `cur` composes a Vue range spanning the synthetic tail. The
/// strict rule (prev must be the LAST token of any kind on its generated line AND reach its
/// source line's true end) rejects the join -> `tsx_range_to_carrier` returns `None`.
#[test]
fn test_tsx_range_line_wrap_over_synthetic_tail_returns_none() {
    // Generated line 0: mapped run [0,3) then a SYNTHETIC (unmapped) token at gen col 3
    // (the tail). Generated line 1: mapped run starting at col 0. Source lines 0 and 1 are
    // each "abc" (len 3), so the line-0 run [0,3) DOES reach its source line end — isolating
    // the failure cause to the synthetic generated tail, not a short source line.
    let json = build_source_map_with_unmapped(
        "App.vue",
        "abc\nabc",
        &[(0, 0, 0, 0), (1, 0, 1, 0)],
        &[(0, 3)], // synthetic tail after the line-0 mapped run
    );
    let mapper = PositionMapper::from_json(&json).unwrap();

    // Both endpoints individually map (start in the line-0 run, end in the line-1 run).
    assert!(
        mapper.tsx_to_carrier(ts(0, 1)).is_some(),
        "precondition: start endpoint maps in the line-0 run"
    );
    assert!(
        mapper.tsx_to_carrier(ts(1, 2)).is_some(),
        "precondition: end endpoint maps in the line-1 run"
    );
    // The line-0 run is NOT the last token on its generated line (a synthetic tail follows
    // at gen col 3), so it must not line-wrap-join the line-1 run -> the range is dropped.
    assert!(
        mapper.tsx_range_to_carrier(ts(0, 1), ts(1, 2)).is_none(),
        "a line-wrap join across a synthetic tail must not compose a range: {:?}",
        mapper.tsx_range_to_carrier(ts(0, 1), ts(1, 2))
    );
}

// ========================================================================
// Multi-run half-open range end. A range whose half-open `end` equals the
// exclusive generated end of a LATER run in the SAME compatibility component
// (not just the start run's own end) must still compose, not be dropped.
// ========================================================================

/// `[1,6)` over two contiguous, compatible runs `[0,3)+[3,6)` has `end = 6` equal to the
/// SECOND run's exclusive generated end. It must map to the correct Vue range.
///
/// Discriminating: a half-open special case that only accepts `end == start_run.dst_end`
/// (here 3) falls through to `tsx_to_carrier(end=6)`, which is `None` (6 is one-past-end of the
/// second run), so the whole multi-token range is wrongly dropped. Accepting `end` at the
/// exclusive end of ANY run in the start's component composes the range.
#[test]
fn test_tsx_range_end_at_later_run_exclusive_end_maps() {
    // Two contiguous mapped runs on gen line 0 mapping to the SAME contiguous source:
    //   gen(0,0)->src(0,0) run [0,3)
    //   gen(0,3)->src(0,3) run [3,6)  (contiguous: A.dst_end 3 == B.dst_col 3, src too)
    //   unmapped gen(0,6) bounds run B to [3,6).
    let json = build_source_map_with_unmapped(
        "App.vue",
        &" ".repeat(40),
        &[(0, 0, 0, 0), (0, 3, 0, 3)],
        &[(0, 6)],
    );
    let mapper = PositionMapper::from_json(&json).unwrap();

    // Precondition: end=6 is one-past-end of the second run, so tsx_to_carrier(6) is None — the
    // range must NOT rely on mapping the exclusive-end position directly.
    assert!(
        mapper.tsx_to_carrier(ts(0, 6)).is_none(),
        "precondition: the exclusive end column itself does not map (one-past run B)"
    );
    // Range [1,6): start inside run A, half-open end at run B's exclusive end -> Vue [1,6).
    let (start, end) = mapper
        .tsx_range_to_carrier(ts(0, 1), ts(0, 6))
        .expect("a range ending at a later compatible run's exclusive end must compose");
    assert_eq!(start, LspPosition::new(0, 1));
    assert_eq!(end, LspPosition::new(0, 6));
}

// ========================================================================
// Source identity in run contiguity. Two runs with identical line/col geometry
// but DIFFERENT source ids must not be treated as contiguous — an LspPosition
// cannot represent which source won, so composing them is meaningless.
// ========================================================================

/// Build a source map with tokens drawn from TWO distinct sources.
/// Each mapped tuple: (dst_line, dst_col, src_line, src_col, source_index) where
/// `source_index` selects which of `sources` the token maps into.
fn build_two_source_map(
    sources: &[(&str, &str)], // (name, content) per source index
    mapped: &[(u32, u32, u32, u32, usize)],
) -> String {
    let mut builder = oxc_sourcemap::SourceMapBuilder::default();
    let ids: Vec<u32> = sources
        .iter()
        .map(|(name, content)| builder.add_source_and_content(name, content))
        .collect();
    let mut all = mapped.to_vec();
    all.sort_by_key(|(dl, dc, _, _, _)| (*dl, *dc));
    for (dl, dc, sl, sc, si) in all {
        builder.add_token(dl, dc, sl, sc, Some(ids[si]), None);
    }
    builder.into_sourcemap().to_json_string()
}

/// Two runs with identical geometry but DIFFERENT `source_id` are in different compatibility
/// components, so a range across them is dropped.
///
/// Discriminating: a contiguity rule that ignores `source_id` joins them (the same-line
/// geometry `prev.dst_end == cur.dst_col && prev.src_end == cur.src_col` holds), and a range
/// across them composes a Vue span whose endpoints come from two unrelated source files — an
/// `LspPosition` cannot represent that. Requiring `prev.source_id == cur.source_id` rejects
/// the join -> `None`.
#[test]
fn test_tsx_range_cross_source_same_geometry_returns_none() {
    // Source 0 ("a.vue", "abcdef") and source 1 ("b.vue", "abcdef"). Run A maps gen(0,0)->
    // src0(0,0) [0,3); run B maps gen(0,3)->src1(0,3) [3,6). The GENERATED + SOURCE
    // line/col geometry is contiguous (A.dst_end 3 == B.dst_col 3; A.src_end 3 == B.src_col
    // 3), but they belong to DIFFERENT source files.
    let json = build_two_source_map(
        &[("a.vue", "abcdef"), ("b.vue", "abcdef")],
        &[(0, 0, 0, 0, 0), (0, 3, 0, 3, 1)],
    );
    let mapper = PositionMapper::from_json(&json).unwrap();

    // Both endpoints individually map.
    assert!(
        mapper.tsx_to_carrier(ts(0, 1)).is_some(),
        "precondition: start endpoint maps (source 0)"
    );
    assert!(
        mapper.tsx_to_carrier(ts(0, 4)).is_some(),
        "precondition: end endpoint maps (source 1)"
    );
    // Geometry matches but the sources differ -> the runs must NOT compose a range.
    assert!(
        mapper.tsx_range_to_carrier(ts(0, 1), ts(0, 4)).is_none(),
        "runs from different sources must not compose a range despite matching geometry: {:?}",
        mapper.tsx_range_to_carrier(ts(0, 1), ts(0, 4))
    );
}

// ========================================================================
// No-`sourcesContent` source-extent inference is proven PER-RUN. The
// `(next_dst_bound, None)` extent-inference arm trusts `next_dst_bound` as the
// source extent ONLY when a per-run lockstep WITNESS — a mapped token sitting
// exactly at the run's generated boundary, same source id + source line, whose
// source column advanced by EXACTLY the generated delta — positively proves it.
// A run whose boundary witness disagrees (or is missing/unmapped) has no proven
// extent and is dropped (maps to None). A neighbouring run that DOES have a
// matching witness still maps: the proof is per-run, not all-or-nothing.
// ========================================================================

/// Per-run granularity of the content-less proof: of two adjacent content-less runs, the one
/// whose boundary witness DISAGREES with its source delta is dropped, while the one whose
/// boundary witness MATCHES still maps. The proof is per-run, never a single global verdict.
///
/// Discriminating: a permissive arm that always trusts `next_dst_bound` as the source extent
/// when content is absent maps the FIRST run's interior to `Some(src ...)`; the per-run witness
/// check rejects it (the boundary token at gen col 3 carries src col 10, not the lockstep
/// `0 + 3 = 3`). Symmetrically, an all-or-nothing global flag that drops EVERY content-less run
/// the moment any same-line pair is non-lockstep would also drop the SECOND run — but the second
/// run's boundary witness (gen col 6 -> src col 13 == `10 + 3`) is exact lockstep, so it must map.
#[test]
fn test_no_sources_content_extent_proof_is_per_run() {
    // No `sourcesContent`. Three tokens on one generated line, one source line:
    //   gen(0,0)->src(0,0)   boundary at gen 3; witness src col 10 != lockstep 0+3=3  -> DROP
    //   gen(0,3)->src(0,10)  boundary at gen 6; witness src col 13 == lockstep 10+3=13 -> MAPS
    //   gen(0,6)->src(0,13)  last-on-line (no following extent signal)               -> dropped
    let json =
        build_source_map_without_content("App.vue", &[(0, 0, 0, 0), (0, 3, 0, 10), (0, 6, 0, 13)]);
    let mapper = PositionMapper::from_json(&json).unwrap();

    // Sanity: the map really is content-less (the arm under test).
    assert!(
        mapper
            .source_map()
            .get_source_contents()
            .all(|c| c.is_none()),
        "precondition: the map carries no sourcesContent"
    );
    // First run: its boundary witness disagrees (src 10 != lockstep 3) -> no proven extent ->
    // dropped, so its would-be interior (col 1) maps to nothing.
    assert!(
        mapper.tsx_to_carrier(ts(0, 1)).is_none(),
        "a content-less run whose boundary witness is non-lockstep must be dropped: {:?}",
        mapper.tsx_to_carrier(ts(0, 1))
    );
    // Second run: its boundary witness (gen 6 -> src 13) advances in EXACT lockstep with the
    // generated delta (3), positively proving the run [3,6) -> [10,13). Its interior (col 4)
    // maps to src col 10 + (4 - 3) = 11. (An all-or-nothing global flag would wrongly drop it.)
    assert_eq!(
        mapper.tsx_to_carrier(ts(0, 4)).unwrap().pos,
        LspPosition::new(0, 11),
        "a content-less run WITH a matching lockstep witness must still map (per-run proof)"
    );
    // The last-on-line token (gen 6) has no following extent signal -> dropped.
    assert!(
        mapper.tsx_to_carrier(ts(0, 6)).is_none(),
        "a last-on-line content-less run has no extent signal and must be dropped: {:?}",
        mapper.tsx_to_carrier(ts(0, 6))
    );
}

/// Companion guard: a content-less map whose tokens ARE position-preserving (lockstep source
/// and generated deltas) still maps its interior runs via the invariant inference — the
/// per-run witness check does not over-reject legitimate position-preserving content-less maps.
/// (This is the in-bounds counterpart that keeps `test_no_sources_content_interior_*` green.)
#[test]
fn test_no_sources_content_position_preserving_still_maps() {
    // Lockstep deltas (gen delta == src delta == 3) -> position-preserving.
    let json =
        build_source_map_without_content("App.vue", &[(0, 0, 0, 0), (0, 3, 0, 3), (0, 6, 0, 6)]);
    let mapper = PositionMapper::from_json(&json).unwrap();
    // Interior run [0,3) still maps under the (valid) invariant inference.
    assert_eq!(
        mapper.tsx_to_carrier(ts(0, 1)).unwrap().pos,
        LspPosition::new(0, 1)
    );
}

/// Build a content-less source map (no `sourcesContent`) that ALSO carries unmapped tokens.
/// Reuses the proven mapped+unmapped encoder, then strips the embedded source content so the
/// decoded map exercises the `(next_dst_bound, None)` content-less extent arm.
fn build_source_map_without_content_with_unmapped(
    source_name: &str,
    mapped: &[(u32, u32, u32, u32)],
    unmapped: &[(u32, u32)],
) -> String {
    let with_content =
        build_source_map_with_unmapped(source_name, &" ".repeat(80), mapped, unmapped);
    let mut value: serde_json::Value = serde_json::from_str(&with_content).unwrap();
    value
        .as_object_mut()
        .unwrap()
        .remove("sourcesContent")
        .expect("builder JSON should carry sourcesContent to strip");
    serde_json::to_string(&value).unwrap()
}

/// A content-less run bounded by an UNMAPPED token has NO lockstep witness at its generated
/// boundary, so its source extent is unproven and the run is DROPPED — an in-run query past the
/// (non-existent) proven prefix maps to nothing.
///
/// Discriminating: with no source content AND no comparable adjacent same-line mapped pair, a
/// vacuous global position-preserving flag passes (`true`), so the OLD arm fabricated
/// `run_len = next_dst_bound` and mapped gen col 4 to `Some(src 0,4)`. The per-run witness check
/// finds only an UNMAPPED token at the boundary (gen col 5), proves nothing, and drops the run.
#[test]
fn test_no_sources_content_run_bounded_by_unmapped_is_dropped() {
    // No `sourcesContent`. One mapped token, then an unmapped token that bounds it at gen 5:
    //   gen(0,0)->src(0,0)   boundary at gen 5 is an UNMAPPED token -> no witness -> DROP
    //   unmapped gen(0,5)
    let json =
        build_source_map_without_content_with_unmapped("App.vue", &[(0, 0, 0, 0)], &[(0, 5)]);
    let mapper = PositionMapper::from_json(&json).unwrap();

    // Sanity: the map really is content-less (the arm under test).
    assert!(
        mapper
            .source_map()
            .get_source_contents()
            .all(|c| c.is_none()),
        "precondition: the map carries no sourcesContent"
    );
    // The boundary token is unmapped -> the run's extent is unproven -> dropped. A vacuously
    // "position-preserving" global flag would instead have fabricated the run [0,5) and mapped
    // col 4 to Some(src 0,4); the per-run witness check forbids that.
    assert!(
        mapper.tsx_to_carrier(ts(0, 4)).is_none(),
        "a content-less run bounded by an unmapped token has no lockstep witness and must be \
         dropped, not fabricated: {:?}",
        mapper.tsx_to_carrier(ts(0, 4))
    );
    // Even the run's own start column maps to nothing (the whole run is dropped).
    assert!(
        mapper.tsx_to_carrier(ts(0, 0)).is_none(),
        "a dropped content-less run covers no columns at all: {:?}",
        mapper.tsx_to_carrier(ts(0, 0))
    );
}

/// A content-less map whose only same-(dst-line)-same-(src-line) witness shows BACKWARD source
/// movement on a co-located generated column must NOT be treated as position-preserving.
/// `saturating_sub` masks that backward move to a zero delta (so a global flag built on it would
/// stay `true` and fabricate extents); the per-run witness uses EXACT, signed equality, so a
/// boundary witness that does not advance by precisely the generated delta proves nothing and the
/// run is dropped.
///
/// Discriminating: the OLD `tokens_are_position_preserving` flag computes
/// `src_delta = b.src_col.saturating_sub(a.src_col)`; for the co-located backward pair
/// `gen(0,2)->src(1,5)` / `gen(0,2)->src(1,1)` both deltas saturate to 0, so the pair fails to
/// disprove PP and the flag returns `true` — the OLD arm then fabricates the first run and maps
/// gen col 1 to `Some(src 0,1)`. The per-run check sees the first run's boundary witnesses (at
/// gen col 2) are on src line 1, not the run's src line 0, so none is a lockstep witness -> the
/// run is dropped -> `None`.
#[test]
fn test_no_sources_content_backward_source_movement_is_rejected() {
    // No `sourcesContent`. The first run is on src line 0; its boundary (gen col 2) holds two
    // co-located mapped tokens on src line 1 whose source columns MOVE BACKWARD (5 -> 1):
    //   gen(0,0)->src(0,0)   boundary at gen 2; witnesses are on src line 1 -> no lockstep
    //   gen(0,2)->src(1,5)   co-located at gen col 2
    //   gen(0,2)->src(1,1)   co-located at gen col 2, source col moves BACKWARD (1 < 5)
    let json =
        build_source_map_without_content("App.vue", &[(0, 0, 0, 0), (0, 2, 1, 5), (0, 2, 1, 1)]);
    let mapper = PositionMapper::from_json(&json).unwrap();

    // Sanity: content-less (the arm under test).
    assert!(
        mapper
            .source_map()
            .get_source_contents()
            .all(|c| c.is_none()),
        "precondition: the map carries no sourcesContent"
    );
    // The first run has no lockstep witness at its boundary (the co-located boundary tokens are
    // on a DIFFERENT source line, and one moves backward). A flag fooled by `saturating_sub`
    // would have fabricated the run and mapped col 1 to Some; the exact per-run check drops it.
    assert!(
        mapper.tsx_to_carrier(ts(0, 1)).is_none(),
        "a backward / cross-src-line co-located witness must not be treated as \
         position-preserving — the content-less run must be dropped: {:?}",
        mapper.tsx_to_carrier(ts(0, 1))
    );
}

// ========================================================================
// `last_on_dst_line` is a TOKEN-ORDER property, not the extent bound. A later
// token at the SAME generated column (which the strictly-greater-column extent
// scan skips) still counts as "follows on this generated line", so a run with
// such a co-located successor must NOT line-wrap-join the next-line run.
// ========================================================================

/// A mapped run is followed by an UNMAPPED token at the SAME generated column on the same line,
/// then a mapped run wraps to line+1 col 0 (source wrapping too). The two mapped runs must land
/// in DIFFERENT compatibility components — the co-located successor means the first run is NOT
/// last-on-its-generated-line, so the line-wrap join is blocked and a range across them drops.
///
/// Discriminating: `last_on_dst_line = next_dst_token_col.is_none()` is `true` here because the
/// extent scan seeks a STRICTLY-greater column and skips the co-located token, inheriting its
/// `None`. That falsely marks the first run last-on-line and lets the line-wrap rule join it to
/// the line-1 run, so a range across them composes (Some). Computing `last_on_dst_line` from
/// token order (the next token shares the generated line) blocks the join -> `None`.
#[test]
fn test_tsx_range_line_wrap_blocked_by_colocated_successor_returns_none() {
    // Generated line 0: mapped run at gen col 3, then an UNMAPPED token co-located at gen col 3
    // (same generated column). Source line 0 is "abcdef" (len 6) so the run [3,6) reaches its
    // source line end (isolating the cause to the co-located successor, not a short src line).
    // Generated line 1: a mapped run wrapping to src line 1 col 0.
    //   mapped   gen(0,3)->src(0,3)   run [3,6) (reaches src line 0 end)
    //   unmapped gen(0,3)             co-located successor on the SAME generated line
    //   mapped   gen(1,0)->src(1,0)   wraps to the next line, col 0, on both sides
    let json = build_source_map_with_unmapped(
        "App.vue",
        "abcdef\nabc",
        &[(0, 3, 0, 3), (1, 0, 1, 0)],
        &[(0, 3)],
    );
    let mapper = PositionMapper::from_json(&json).unwrap();

    // Both endpoints individually map (start inside the line-0 run, end inside the line-1 run).
    assert!(
        mapper.tsx_to_carrier(ts(0, 4)).is_some(),
        "precondition: start endpoint maps in the line-0 run"
    );
    assert!(
        mapper.tsx_to_carrier(ts(1, 1)).is_some(),
        "precondition: end endpoint maps in the line-1 run"
    );
    // The line-0 run has a co-located successor at gen col 3, so it is NOT last-on-its-line and
    // must not line-wrap-join the line-1 run -> the range across them is dropped.
    assert!(
        mapper.tsx_range_to_carrier(ts(0, 4), ts(1, 1)).is_none(),
        "a line-wrap join across a co-located same-column successor must not compose a range: {:?}",
        mapper.tsx_range_to_carrier(ts(0, 4), ts(1, 1))
    );
}
