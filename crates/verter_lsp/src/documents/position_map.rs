use oxc_sourcemap::OwnedSourceMap;
use verter_span::{LspPosition, TsPosition};

/// Bidirectional position mapper between Vue source positions and generated IDE positions.
///
/// Consumes an `oxc_sourcemap::OwnedSourceMap` (from `verter_session.get_ide()`) and provides
/// strict, **in-run** lookups in both directions:
/// - [`tsx_to_vue`](PositionMapper::tsx_to_vue): generated TSX position ([`TsPosition`]) ->
///   original Vue position ([`LspPosition`], wrapped in [`SourceMapped`]).
/// - [`vue_to_tsx`](PositionMapper::vue_to_tsx): original Vue position ([`LspPosition`]) ->
///   generated TSX position ([`TsPosition`], wrapped in [`GeneratedMapped`]).
///
/// Both directions return `None` unless the query falls **strictly inside a single mapped
/// token's run**. There is NO cross-token extrapolation and NO snap-to-nearest: a query in
/// unmapped/synthetic content, or in a gap between tokens, or that would bridge into the next
/// token, maps to nothing. Character-level precision is preserved ONLY within one mapped run
/// (the query's offset from the run start is added to the run's mapped start). A range maps
/// only when BOTH endpoints independently resolve inside compatible mapped runs.
///
/// All positions use 0-indexed lines and UTF-16 columns (matching LSP `Position`). The typed
/// [`TsPosition`] / [`LspPosition`] wrappers make it impossible to pass a TSX coordinate where
/// a Vue coordinate is expected.
#[derive(Clone)]
pub struct PositionMapper {
    map: OwnedSourceMap,
}

/// Result of [`PositionMapper::tsx_to_vue`]: an original-`.vue` position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceMapped {
    /// The resolved Vue source position (LSP-negotiated encoding).
    pub pos: LspPosition,
}

/// Result of [`PositionMapper::vue_to_tsx`]: a generated-TSX position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GeneratedMapped {
    /// The resolved generated-TSX position.
    pub pos: TsPosition,
}

impl PositionMapper {
    /// Create a position mapper from a source map JSON string.
    pub fn from_json(json: &str) -> Result<Self, String> {
        let map = OwnedSourceMap::from_json_string(json)
            .map_err(|e| format!("invalid source map: {e}"))?;
        Ok(Self { map })
    }

    /// Map a generated TSX position back to the original Vue source position.
    ///
    /// This is the most common LSP operation: TSGO or TypeScript reports a position
    /// in the generated TSX, and we need the corresponding Vue source position.
    ///
    /// Strict in-run lookup: returns `Some` ONLY when the query lies strictly inside a
    /// single mapped token's generated run. A query on an unmapped/synthetic token (e.g.
    /// the `_ctx.` / `$setup.` prefix), in a gap between tokens, or bridging into the next
    /// token returns `None`. Within the run, character precision is preserved by adding the
    /// query's offset from the run start to the mapped source column.
    pub fn tsx_to_vue(&self, pos: TsPosition) -> Option<SourceMapped> {
        let line = pos.line;
        let column = pos.character;

        let lookup_table = self.map.generate_lookup_table();
        let token = self.map.lookup_token(&lookup_table, line, column)?;

        // The covering token must itself be mapped. A query that lands on an
        // unmapped / `Inserted` token (synthetic text such as `_ctx.` / `$setup.`)
        // has no source position -> None.
        token.get_source_id()?;

        // In-run containment: the query must lie strictly inside THIS token's mapped
        // run on this generated line. The run ends where the next token on the same
        // generated line starts (or at end-of-line if none). `lookup_token` already
        // guarantees the token covers the query, but we re-assert the lower bound and
        // reject any query that bridges into the next/unmapped token's run.
        let dst_col = token.get_dst_col();
        if token.get_dst_line() != line || column < dst_col {
            return None;
        }
        if let Some(next_dst_col) = self.next_dst_col_on_line(line, dst_col) {
            if column >= next_dst_col {
                return None;
            }
        }

        // Within-run character precision: Original chunks are byte-identical between
        // source and generated, so adding the in-run offset to the mapped source column
        // is exact. This delta is applied ONLY here, inside one mapped run.
        Some(SourceMapped {
            pos: LspPosition {
                line: token.get_src_line(),
                character: token.get_src_col() + (column - dst_col),
            },
        })
    }

    /// Map an original Vue source position to the generated TSX position.
    ///
    /// This is needed when the user interacts at a Vue position and we need
    /// to query TSGO at the corresponding TSX offset.
    ///
    /// Strict in-run lookup (no snap-to-previous): returns `Some` ONLY when the target
    /// source position lies strictly inside a single mapped token's source run — i.e. the
    /// run starts at or before the target and the next token on the same source line starts
    /// strictly after the target. A target in a gap, or in a later/unmapped run, returns
    /// `None`. Within the run, the in-run offset is added to the mapped generated column.
    pub fn vue_to_tsx(&self, pos: LspPosition) -> Option<GeneratedMapped> {
        let line = pos.line;
        let column = pos.character;

        // Find the single mapped token whose own source run contains the target:
        // `src_col <= column` and the next source token on this line starts strictly
        // after `column`. This is interval containment, not a closest-preceding snap —
        // a target that falls past a token's run (into a gap or a later run) matches no
        // token and returns `None`.
        for token in self.map.get_tokens() {
            if token.get_source_id().is_none() {
                continue;
            }
            if token.get_src_line() != line {
                continue;
            }
            let src_col = token.get_src_col();
            if column < src_col {
                continue;
            }
            let run_end = self.next_src_col_on_line(line, src_col);
            let contained = match run_end {
                Some(next_src_col) => column < next_src_col,
                None => true, // last mapped run on the line extends to end-of-line
            };
            if contained {
                return Some(GeneratedMapped {
                    pos: TsPosition {
                        line: token.get_dst_line(),
                        character: token.get_dst_col() + (column - src_col),
                    },
                });
            }
        }

        None
    }

    /// Smallest generated column strictly greater than `dst_col` among tokens on
    /// generated `line`. `None` means `dst_col` starts the last run on the line (the run
    /// extends to end-of-line). Used to bound a mapped run for in-run containment; it
    /// considers ALL tokens (mapped and unmapped) so a run never bleeds into synthetic
    /// (`Inserted`) content that follows it on the same generated line.
    fn next_dst_col_on_line(&self, line: u32, dst_col: u32) -> Option<u32> {
        self.map
            .get_tokens()
            .filter(|t| t.get_dst_line() == line && t.get_dst_col() > dst_col)
            .map(|t| t.get_dst_col())
            .min()
    }

    /// Smallest source column strictly greater than `src_col` among mapped tokens on
    /// source `line`. `None` means `src_col` starts the last mapped run on the line.
    /// Used to bound a mapped source run for in-run containment.
    fn next_src_col_on_line(&self, line: u32, src_col: u32) -> Option<u32> {
        self.map
            .get_tokens()
            .filter(|t| {
                t.get_source_id().is_some() && t.get_src_line() == line && t.get_src_col() > src_col
            })
            .map(|t| t.get_src_col())
            .min()
    }

    /// Get the underlying source map (for advanced queries).
    pub fn source_map(&self) -> &OwnedSourceMap {
        &self.map
    }
}

#[cfg(test)]
#[allow(clippy::type_complexity)]
mod tests {
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
        assert!(mapper.tsx_to_vue(ts(0, 0)).is_none());
    }

    // ========================================================================
    // tsx_to_vue (generated -> source)
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

        let m = mapper.tsx_to_vue(ts(0, 0)).unwrap();
        assert_eq!(m.pos, LspPosition::new(2, 5));

        let m = mapper.tsx_to_vue(ts(1, 0)).unwrap();
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
        let json = build_source_map_with_unmapped(
            "App.vue",
            "const x = hello;",
            &[(0, 0, 0, 0)],
            &[(0, 5)],
        );
        let mapper = PositionMapper::from_json(&json).unwrap();

        // Query 5 columns past the mapped token, on the unmapped token -> None.
        assert!(
            mapper.tsx_to_vue(ts(0, 5)).is_none(),
            "must not extrapolate across an unmapped-token boundary: {:?}",
            mapper.tsx_to_vue(ts(0, 5))
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
        let m = mapper.tsx_to_vue(ts(0, 3)).unwrap();
        assert_eq!(m.pos, LspPosition::new(0, 13));
        // Run start maps exactly.
        assert_eq!(
            mapper.tsx_to_vue(ts(0, 0)).unwrap().pos,
            LspPosition::new(0, 10)
        );
        // Last in-run column (5) maps; column 6 is the unmapped boundary -> None.
        assert_eq!(
            mapper.tsx_to_vue(ts(0, 5)).unwrap().pos,
            LspPosition::new(0, 15)
        );
        assert!(mapper.tsx_to_vue(ts(0, 6)).is_none());
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
        assert!(mapper.tsx_to_vue(ts(0, 3)).is_none());
        assert!(mapper.tsx_to_vue(ts(0, 5)).is_none());
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
        assert!(mapper.tsx_to_vue(ts(0, 5)).is_none());
        // The first mapped run is bounded to [0,3): col 2 still maps within it.
        assert_eq!(
            mapper.tsx_to_vue(ts(0, 2)).unwrap().pos,
            LspPosition::new(0, 2)
        );
        // The second mapped run starts at gen 8.
        assert_eq!(
            mapper.tsx_to_vue(ts(0, 8)).unwrap().pos,
            LspPosition::new(0, 20)
        );
    }

    // ========================================================================
    // vue_to_tsx (source -> generated)
    // ========================================================================

    #[test]
    fn test_vue_to_tsx_exact_token_match() {
        // gen(5,0) -> src(0,0), gen(6,4) -> src(1,2)
        let json =
            build_test_source_map("App.vue", "hello\n  world", &[(5, 0, 0, 0), (6, 4, 1, 2)]);
        let mapper = PositionMapper::from_json(&json).unwrap();

        assert_eq!(
            mapper.vue_to_tsx(vue(0, 0)).unwrap().pos,
            TsPosition::new(5, 0)
        );
        assert_eq!(
            mapper.vue_to_tsx(vue(1, 2)).unwrap().pos,
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
            mapper.vue_to_tsx(vue(5, 0)).is_none(),
            "unmapped source line must not snap to a preceding token: {:?}",
            mapper.vue_to_tsx(vue(5, 0))
        );
        // src(0,0) still maps exactly.
        assert_eq!(
            mapper.vue_to_tsx(vue(0, 0)).unwrap().pos,
            TsPosition::new(0, 0)
        );
    }

    /// Within-run character precision for `vue_to_tsx`: a target inside a single mapped
    /// source run maps to the corresponding generated column.
    #[test]
    fn test_vue_to_tsx_within_run_character_precision() {
        // Token tuples are (dst_line, dst_col, src_line, src_col):
        //   gen(0,10) -> src(0,7)  — start of the source run
        //   gen(0,40) -> src(0,20) — next mapped token, bounds the run to src [7,20)
        let json =
            build_test_source_map("App.vue", &" ".repeat(40), &[(0, 10, 0, 7), (0, 40, 0, 20)]);
        let mapper = PositionMapper::from_json(&json).unwrap();

        // src col 13 -> gen col 10 + (13-7) = 16.
        assert_eq!(
            mapper.vue_to_tsx(vue(0, 13)).unwrap().pos,
            TsPosition::new(0, 16)
        );
        // Run start: src 7 -> gen 10.
        assert_eq!(
            mapper.vue_to_tsx(vue(0, 7)).unwrap().pos,
            TsPosition::new(0, 10)
        );
        // Last in-run col (19) -> gen 22; col 20 enters the next run (maps via that token).
        assert_eq!(
            mapper.vue_to_tsx(vue(0, 19)).unwrap().pos,
            TsPosition::new(0, 22)
        );
        assert_eq!(
            mapper.vue_to_tsx(vue(0, 20)).unwrap().pos,
            TsPosition::new(0, 40)
        );
    }

    #[test]
    fn test_vue_to_tsx_no_tokens() {
        let json = build_test_source_map("App.vue", "", &[]);
        let mapper = PositionMapper::from_json(&json).unwrap();
        assert!(mapper.vue_to_tsx(vue(0, 0)).is_none());
    }

    #[test]
    fn test_vue_to_tsx_position_before_all_tokens() {
        // All tokens start at src(2,0) or later; query before them -> None.
        let json = build_test_source_map("App.vue", "\n\nhello", &[(5, 0, 2, 0)]);
        let mapper = PositionMapper::from_json(&json).unwrap();
        assert!(mapper.vue_to_tsx(vue(0, 0)).is_none());
    }

    #[test]
    fn test_vue_to_tsx_multiline_source() {
        // Script block at Vue lines 5/6 mapped to TSX lines 0/1.
        let json = build_test_source_map(
            "App.vue",
            "<template>\n  <div/>\n</template>\n\n<script setup>\nconst x = 1;\nconst y = 2;\n</script>",
            &[(0, 0, 5, 0), (1, 0, 6, 0)],
        );
        let mapper = PositionMapper::from_json(&json).unwrap();

        // src(5,6) -> gen(0,6): "x" in "const x = 1;" (run extends to EOL on line 5).
        assert_eq!(
            mapper.vue_to_tsx(vue(5, 6)).unwrap().pos,
            TsPosition::new(0, 6)
        );
        // src(6,6) -> gen(1,6).
        assert_eq!(
            mapper.vue_to_tsx(vue(6, 6)).unwrap().pos,
            TsPosition::new(1, 6)
        );
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

        let tsx = mapper.vue_to_tsx(vue(0, 0)).unwrap().pos;
        let back = mapper.tsx_to_vue(ts(tsx.line, tsx.character)).unwrap().pos;
        assert_eq!(back, LspPosition::new(0, 0));

        let tsx = mapper.vue_to_tsx(vue(1, 0)).unwrap().pos;
        let back = mapper.tsx_to_vue(ts(tsx.line, tsx.character)).unwrap().pos;
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
            let tsx = mapper.vue_to_tsx(vue(3, vue_col)).unwrap().pos;
            assert_eq!(tsx, TsPosition::new(0, 7 + i), "forward char {i}");

            let back = mapper.tsx_to_vue(ts(tsx.line, tsx.character)).unwrap().pos;
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
            mapper.tsx_to_vue(ts(0, 3)).unwrap().pos,
            LspPosition::new(0, 20)
        );
        // One past the end of the (bounded) second run: gen col 6 is the unmapped
        // boundary -> None.
        assert!(mapper.tsx_to_vue(ts(0, 6)).is_none());
        // Interior of the second run still maps.
        assert_eq!(
            mapper.tsx_to_vue(ts(0, 5)).unwrap().pos,
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
            mapper.vue_to_tsx(vue(1, 3)).unwrap().pos,
            TsPosition::new(0, 7)
        );
        // middle ('u', col 5) -> TSX col 9
        assert_eq!(
            mapper.vue_to_tsx(vue(1, 5)).unwrap().pos,
            TsPosition::new(0, 9)
        );
        // end ('t', col 7) -> TSX col 11
        assert_eq!(
            mapper.vue_to_tsx(vue(1, 7)).unwrap().pos,
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
            mapper.tsx_to_vue(ts(0, 7)).unwrap().pos,
            LspPosition::new(1, 3)
        );
        // TSX col 9 ('u') -> Vue(1,5)
        assert_eq!(
            mapper.tsx_to_vue(ts(0, 9)).unwrap().pos,
            LspPosition::new(1, 5)
        );
        // TSX col 11 ('t') -> Vue(1,7)
        assert_eq!(
            mapper.tsx_to_vue(ts(0, 11)).unwrap().pos,
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
            mapper.vue_to_tsx(vue(1, 9)).unwrap().pos,
            TsPosition::new(0, 7)
        );
        assert_eq!(
            mapper.vue_to_tsx(vue(1, 18)).unwrap().pos,
            TsPosition::new(0, 16)
        );
        assert_eq!(
            mapper.tsx_to_vue(ts(0, 7)).unwrap().pos,
            LspPosition::new(1, 9)
        );
        assert_eq!(
            mapper.tsx_to_vue(ts(0, 16)).unwrap().pos,
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
            mapper.vue_to_tsx(vue(1, 6)).unwrap().pos,
            TsPosition::new(0, 6)
        );
        assert_eq!(
            mapper.vue_to_tsx(vue(1, 8)).unwrap().pos,
            TsPosition::new(0, 8)
        );
        let back = mapper.tsx_to_vue(ts(0, 8)).unwrap().pos;
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
            mapper.vue_to_tsx(vue(0, 8)).unwrap().pos,
            TsPosition::new(0, 2)
        );
        // within-run interior after the tabs
        assert_eq!(
            mapper.vue_to_tsx(vue(0, 10)).unwrap().pos,
            TsPosition::new(0, 4)
        );
        let back = mapper.tsx_to_vue(ts(0, 4)).unwrap().pos;
        assert_eq!(back, LspPosition::new(0, 10));
    }

    /// A mapped expression spanning multiple TSX lines: each generated line maps back to
    /// its own source line, and a roundtrip on each line is exact.
    #[test]
    fn test_multiline_mapped_expression() {
        // gen line 0 -> src line 4, gen line 1 -> src line 5, gen line 2 -> src line 6.
        let json = build_test_source_map(
            "App.vue",
            &"x\n".repeat(10),
            &[
                (0, 0, 4, 0),
                (0, 5, 4, 5),
                (1, 0, 5, 0),
                (1, 5, 5, 5),
                (2, 0, 6, 0),
            ],
        );
        let mapper = PositionMapper::from_json(&json).unwrap();

        // Each generated line maps to its source line, within-run column preserved.
        assert_eq!(
            mapper.tsx_to_vue(ts(0, 2)).unwrap().pos,
            LspPosition::new(4, 2)
        );
        assert_eq!(
            mapper.tsx_to_vue(ts(1, 3)).unwrap().pos,
            LspPosition::new(5, 3)
        );
        // gen(2,*) is the last mapped run -> extends to EOL on gen line 2.
        assert_eq!(
            mapper.tsx_to_vue(ts(2, 4)).unwrap().pos,
            LspPosition::new(6, 4)
        );

        // Roundtrip on the middle line.
        let tsx = mapper.vue_to_tsx(vue(5, 3)).unwrap().pos;
        assert_eq!(tsx, TsPosition::new(1, 3));
        assert_eq!(
            mapper.tsx_to_vue(ts(tsx.line, tsx.character)).unwrap().pos,
            LspPosition::new(5, 3)
        );
    }

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
            mapper.vue_to_tsx(vue(0, 5)).unwrap().pos,
            TsPosition::new(0, 7)
        );
        assert_eq!(
            mapper.vue_to_tsx(vue(0, 6)).unwrap().pos,
            TsPosition::new(0, 8)
        );
        assert_eq!(
            mapper.vue_to_tsx(vue(0, 7)).unwrap().pos,
            TsPosition::new(0, 9)
        );
    }

    #[test]
    fn test_utf16_emoji_before_binding_reverse() {
        let json =
            build_source_map_with_unmapped("App.vue", "😀{{ msg }}", &[(0, 7, 0, 5)], &[(0, 0)]);
        let mapper = PositionMapper::from_json(&json).unwrap();

        assert_eq!(
            mapper.tsx_to_vue(ts(0, 7)).unwrap().pos,
            LspPosition::new(0, 5)
        );
        assert_eq!(
            mapper.tsx_to_vue(ts(0, 8)).unwrap().pos,
            LspPosition::new(0, 6)
        );
        assert_eq!(
            mapper.tsx_to_vue(ts(0, 9)).unwrap().pos,
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
            let tsx = mapper.vue_to_tsx(vue(0, vue_col)).unwrap().pos;
            assert_eq!(tsx.character, 5 + i, "café[{i}] forward");
            let back = mapper.tsx_to_vue(ts(tsx.line, tsx.character)).unwrap().pos;
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
        assert_eq!(mapper.vue_to_tsx(vue(0, 3)).unwrap().pos.character, 5);
        assert_eq!(mapper.tsx_to_vue(ts(0, 5)).unwrap().pos.character, 3);
        // 😀 first unit col 4 -> gen 6
        assert_eq!(mapper.vue_to_tsx(vue(0, 4)).unwrap().pos.character, 6);
        assert_eq!(mapper.tsx_to_vue(ts(0, 6)).unwrap().pos.character, 4);
        // 😀 second unit col 5 -> gen 7
        assert_eq!(mapper.vue_to_tsx(vue(0, 5)).unwrap().pos.character, 7);
        assert_eq!(mapper.tsx_to_vue(ts(0, 7)).unwrap().pos.character, 5);
        // 'b' col 6 -> gen 8
        assert_eq!(mapper.vue_to_tsx(vue(0, 6)).unwrap().pos.character, 8);
        assert_eq!(mapper.tsx_to_vue(ts(0, 8)).unwrap().pos.character, 6);
    }
}
