use oxc_sourcemap::OwnedSourceMap;
use verter_span::{LspPosition, TsPosition};

/// A single mapped run, precomputed once at [`PositionMapper`] construction.
///
/// A run is one mapped source-map token's contiguous mapped extent. Because `.vue` eval
/// sources are **position-preserving** (`IndexedReady.eval_source`), the generated and
/// source extents of a run have the SAME length: `dst_end - dst_col == src_end - src_col`.
///
/// The run's length is the TRUE extent of the token's mapped content — NOT "to the next
/// mapped token" and NOT "to end-of-line". It is bounded by both:
///  - the next token of ANY kind on the same generated line (so an unmapped/synthetic
///    token immediately after the run caps it, and a gap to the next *mapped* token is
///    NOT swallowed); and
///  - the token's own source line's true content length (so a last-on-line run cannot
///    extend past the real source text into trailing synthetic suffix / EOL).
///
/// The minimum of the two is the run length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MappedRun {
    dst_line: u32,
    dst_col: u32,
    /// Exclusive generated-column end of the run on `dst_line`.
    dst_end: u32,
    src_line: u32,
    src_col: u32,
    /// Exclusive source-column end of the run on `src_line` (`src_col + run_len`).
    src_end: u32,
}

impl MappedRun {
    #[inline]
    fn dst_contains(&self, line: u32, col: u32) -> bool {
        line == self.dst_line && col >= self.dst_col && col < self.dst_end
    }

    #[inline]
    fn src_contains(&self, line: u32, col: u32) -> bool {
        line == self.src_line && col >= self.src_col && col < self.src_end
    }
}

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
/// run** (a `MappedRun` precomputed at construction). There is NO cross-token extrapolation
/// and NO snap-to-nearest: a query in unmapped/synthetic content, in a gap between runs, or
/// that would bridge into the next run, maps to nothing. Character-level precision is
/// preserved ONLY within one mapped run (the query's offset from the run start is added to
/// the run's mapped start).
///
/// A RANGE maps only via [`tsx_range_to_vue`](PositionMapper::tsx_range_to_vue), which
/// requires both endpoints to resolve inside **compatible** runs (the same run, or
/// genuinely-contiguous mapped runs with no synthetic/unmapped content between them). Two
/// endpoints that each map but lie in runs separated by synthetic content do NOT compose.
///
/// All positions use 0-indexed lines and UTF-16 columns (matching LSP `Position`). The typed
/// [`TsPosition`] / [`LspPosition`] wrappers make it impossible to pass a TSX coordinate where
/// a Vue coordinate is expected.
///
/// Performance: the mapped runs and the per-line sorted indices used for lookup are
/// precomputed ONCE in [`from_json`](PositionMapper::from_json). Both public lookups are then
/// O(log n) (binary search within a line) + O(1) bound — no per-call table rebuild and no
/// quadratic re-scan, which matters on hot per-token paths (semantic tokens, inlay hints).
#[derive(Clone)]
pub struct PositionMapper {
    map: OwnedSourceMap,
    /// All mapped runs, stored in `(dst_line, dst_col)` order (the source-map token order).
    /// A run's stable identity is its index into this vec (used for endpoint-compatibility).
    runs: Vec<MappedRun>,
    /// Per generated line: indices into `runs`, sorted by `dst_col`. Index = dst line.
    by_dst_line: Vec<Vec<u32>>,
    /// Per source line: indices into `runs`, sorted by `src_col`. Index = src line.
    by_src_line: Vec<Vec<u32>>,
}

/// Result of [`PositionMapper::tsx_to_vue`]: an original-`.vue` position plus the identity of
/// the mapped run it resolved inside (so a range composer can prove endpoint compatibility).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceMapped {
    /// The resolved Vue source position (LSP-negotiated encoding).
    pub pos: LspPosition,
    /// Stable identity of the mapped run the query resolved inside.
    pub run: RunId,
}

/// Result of [`PositionMapper::vue_to_tsx`]: a generated-TSX position plus the identity of the
/// mapped run it resolved inside (so a range composer can prove endpoint compatibility).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GeneratedMapped {
    /// The resolved generated-TSX position.
    pub pos: TsPosition,
    /// Stable identity of the mapped run the query resolved inside.
    pub run: RunId,
}

/// Stable identity of a single mapped run within one [`PositionMapper`].
///
/// It is opaque on purpose: callers compare two `RunId`s only through
/// [`PositionMapper::tsx_range_to_vue`] (which decides endpoint compatibility), never by
/// reaching into the underlying index. Two `RunId`s from DIFFERENT mappers are meaningless
/// together.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RunId(u32);

impl PositionMapper {
    /// Create a position mapper from a source map JSON string.
    ///
    /// Precomputes the mapped runs (true-extent bounded) and the per-line sorted lookup
    /// indices once, so subsequent lookups are O(log n) without rebuilding any table.
    pub fn from_json(json: &str) -> Result<Self, String> {
        let map = OwnedSourceMap::from_json_string(json)
            .map_err(|e| format!("invalid source map: {e}"))?;
        let (runs, by_dst_line, by_src_line) = Self::precompute_runs(&map);
        Ok(Self {
            map,
            runs,
            by_dst_line,
            by_src_line,
        })
    }

    /// Build the mapped runs and per-line indices from the source map's tokens.
    ///
    /// Tokens arrive sorted by `(dst_line, dst_col)` (the source-map invariant). Each MAPPED
    /// token starts a run whose true length is `min(next-dst-token-delta, source-line-
    /// remaining)` — see [`MappedRun`]. Unmapped tokens produce no run but still bound the
    /// preceding run (their `dst_col` is the next-dst-token boundary).
    #[allow(clippy::type_complexity)]
    fn precompute_runs(map: &OwnedSourceMap) -> (Vec<MappedRun>, Vec<Vec<u32>>, Vec<Vec<u32>>) {
        let tokens: Vec<_> = map.get_tokens().collect();

        // UTF-16 length of each line of each source, indexed [source_id][line].
        let source_line_lens: Vec<Vec<u32>> = {
            let mut per_source = Vec::new();
            for content in map.get_source_contents() {
                let lens = content
                    .map(|c| c.split('\n').map(utf16_line_len).collect::<Vec<u32>>())
                    .unwrap_or_default();
                per_source.push(lens);
            }
            per_source
        };
        let src_line_len = |source_id: u32, line: u32| -> Option<u32> {
            source_line_lens
                .get(source_id as usize)
                .and_then(|lens| lens.get(line as usize))
                .copied()
        };

        let mut runs: Vec<MappedRun> = Vec::new();
        for (idx, token) in tokens.iter().enumerate() {
            let Some(source_id) = token.get_source_id() else {
                continue; // unmapped token: no run (it only bounds the preceding run)
            };
            let dst_line = token.get_dst_line();
            let dst_col = token.get_dst_col();

            // Bound 1: the next token of ANY kind on the same generated line. A gap to the
            // next mapped token is therefore NOT swallowed (an unmapped token in between, or
            // the next mapped token itself, caps this run at its own start).
            let next_dst_bound = tokens
                .iter()
                .skip(idx + 1)
                .take_while(|t| t.get_dst_line() == dst_line)
                .map(|t| t.get_dst_col())
                .find(|&c| c > dst_col)
                .map(|c| c - dst_col);

            // Bound 2: the token's own source line's true content length remaining. This
            // caps a last-on-line run so it cannot extend past real source text into a
            // synthetic suffix or to EOL.
            let src_col = token.get_src_col();
            let src_remaining = src_line_len(source_id, token.get_src_line())
                .map(|len| len.saturating_sub(src_col));

            let run_len = match (next_dst_bound, src_remaining) {
                (Some(a), Some(b)) => a.min(b),
                (Some(a), None) => a,
                (None, Some(b)) => b,
                // No following token and no source-line info: degenerate map with a single
                // run and no source content. Nothing to bound it; it covers no columns.
                (None, None) => 0,
            };

            if run_len == 0 {
                continue;
            }

            runs.push(MappedRun {
                dst_line,
                dst_col,
                dst_end: dst_col + run_len,
                src_line: token.get_src_line(),
                src_col,
                src_end: src_col + run_len,
            });
        }

        // Per-line indices. `runs` is already in (dst_line, dst_col) order, so by_dst_line is
        // naturally sorted by dst_col; by_src_line must be explicitly sorted by src_col.
        let max_dst_line = runs.iter().map(|r| r.dst_line).max();
        let max_src_line = runs.iter().map(|r| r.src_line).max();

        let mut by_dst_line: Vec<Vec<u32>> = match max_dst_line {
            Some(m) => vec![Vec::new(); m as usize + 1],
            None => Vec::new(),
        };
        let mut by_src_line: Vec<Vec<u32>> = match max_src_line {
            Some(m) => vec![Vec::new(); m as usize + 1],
            None => Vec::new(),
        };
        for (i, run) in runs.iter().enumerate() {
            by_dst_line[run.dst_line as usize].push(i as u32);
            by_src_line[run.src_line as usize].push(i as u32);
        }
        for line in &mut by_src_line {
            line.sort_by_key(|&i| runs[i as usize].src_col);
        }

        (runs, by_dst_line, by_src_line)
    }

    /// Find the mapped run whose GENERATED extent contains `(line, col)`, by binary search
    /// within the line's runs (sorted by `dst_col`). O(log n).
    fn run_at_dst(&self, line: u32, col: u32) -> Option<RunId> {
        let line_runs = self.by_dst_line.get(line as usize)?;
        // greatest run whose dst_col <= col, then verify in-run containment (col < dst_end).
        let pos = line_runs.partition_point(|&i| self.runs[i as usize].dst_col <= col);
        let run_idx = *line_runs.get(pos.checked_sub(1)?)?;
        let run = &self.runs[run_idx as usize];
        if run.dst_contains(line, col) {
            Some(RunId(run_idx))
        } else {
            None
        }
    }

    /// Find the mapped run whose SOURCE extent contains `(line, col)`, by binary search
    /// within the line's runs (sorted by `src_col`). O(log n).
    fn run_at_src(&self, line: u32, col: u32) -> Option<RunId> {
        let line_runs = self.by_src_line.get(line as usize)?;
        let pos = line_runs.partition_point(|&i| self.runs[i as usize].src_col <= col);
        let run_idx = *line_runs.get(pos.checked_sub(1)?)?;
        let run = &self.runs[run_idx as usize];
        if run.src_contains(line, col) {
            Some(RunId(run_idx))
        } else {
            None
        }
    }

    /// Map a generated TSX position back to the original Vue source position.
    ///
    /// This is the most common LSP operation: TSGO or TypeScript reports a position
    /// in the generated TSX, and we need the corresponding Vue source position.
    ///
    /// Strict in-run lookup: returns `Some` ONLY when the query lies strictly inside a
    /// single mapped run's generated extent. A query on an unmapped/synthetic token (e.g.
    /// the `_ctx.` / `$setup.` prefix), in a gap between runs, past the run's true content
    /// end, or bridging into the next run returns `None`. Within the run, character precision
    /// is preserved by adding the query's offset from the run start to the mapped source
    /// column.
    pub fn tsx_to_vue(&self, pos: TsPosition) -> Option<SourceMapped> {
        let run_id = self.run_at_dst(pos.line, pos.character)?;
        let run = &self.runs[run_id.0 as usize];
        // Within-run character precision: the run's generated and source extents are
        // byte-identical (position-preserving), so adding the in-run offset to the mapped
        // source column is exact. This delta is applied ONLY here, inside one mapped run.
        Some(SourceMapped {
            pos: LspPosition {
                line: run.src_line,
                character: run.src_col + (pos.character - run.dst_col),
            },
            run: run_id,
        })
    }

    /// Map an original Vue source position to the generated TSX position.
    ///
    /// This is needed when the user interacts at a Vue position and we need
    /// to query TSGO at the corresponding TSX offset.
    ///
    /// Strict in-run lookup (no snap-to-previous): returns `Some` ONLY when the target
    /// source position lies strictly inside a single mapped run's source extent. A target in
    /// a gap, past the run's true content end, or in a later/unmapped run returns `None`.
    /// Within the run, the in-run offset is added to the mapped generated column.
    pub fn vue_to_tsx(&self, pos: LspPosition) -> Option<GeneratedMapped> {
        let run_id = self.run_at_src(pos.line, pos.character)?;
        let run = &self.runs[run_id.0 as usize];
        Some(GeneratedMapped {
            pos: TsPosition {
                line: run.dst_line,
                character: run.dst_col + (pos.character - run.src_col),
            },
            run: run_id,
        })
    }

    /// Map a generated TSX **range** `[start, end)` back to a Vue source range `(start, end)`,
    /// enforcing the half-open endpoint-compatibility rule.
    ///
    /// A range maps only when both endpoints resolve inside **compatible** mapped runs:
    ///  - the `start` position resolves inside some run `S`; and
    ///  - the half-open `end` either (a) equals `S`'s exclusive generated end exactly (the
    ///    range terminates at `S`'s mapped end — same run), or (b) resolves inside a run `E`
    ///    that is contiguous with `S` (`S.dst_end == E.dst_col`, i.e. NO synthetic/unmapped
    ///    content between them on the line).
    ///
    /// Any range whose endpoints fall in runs separated by synthetic/unmapped content, on
    /// different lines, or otherwise incompatible, returns `None` — never a bogus straddling
    /// range. This is the binding "both endpoints inside compatible mapped spans" contract.
    pub fn tsx_range_to_vue(
        &self,
        start: TsPosition,
        end: TsPosition,
    ) -> Option<(LspPosition, LspPosition)> {
        let start_mapped = self.tsx_to_vue(start)?;
        let start_run = &self.runs[start_mapped.run.0 as usize];

        // Half-open end exactly at the start run's exclusive generated end: the range
        // terminates at this run's mapped content end (same run, no bridging).
        if end.line == start_run.dst_line && end.character == start_run.dst_end {
            return Some((
                start_mapped.pos,
                LspPosition {
                    line: start_run.src_line,
                    character: start_run.src_end,
                },
            ));
        }

        // Otherwise the end must resolve inside a run compatible with the start run.
        let end_mapped = self.tsx_to_vue(end)?;
        if self.runs_compatible(start_mapped.run, end_mapped.run) {
            Some((start_mapped.pos, end_mapped.pos))
        } else {
            None
        }
    }

    /// Whether two mapped runs are COMPATIBLE for composing a range: the same run, or a
    /// chain of genuinely-contiguous runs on the same generated line with NO synthetic /
    /// unmapped content between them.
    ///
    /// Contiguity is decided in generated space: run `A` is directly followed by run `B`
    /// iff `A.dst_end == B.dst_col`. Because a run's `dst_end` is bounded by the next token
    /// of ANY kind, an unmapped/synthetic token sitting between `A` and `B` forces
    /// `A.dst_end < B.dst_col` — so any synthetic content between the runs breaks the chain
    /// and the range is rejected.
    fn runs_compatible(&self, a: RunId, b: RunId) -> bool {
        if a == b {
            return true;
        }
        let (lo, hi) = if a.0 <= b.0 { (a, b) } else { (b, a) };
        let lo_line = self.runs[lo.0 as usize].dst_line;
        // Walk the dst-ordered run indices from lo..=hi; every adjacent pair must be exactly
        // contiguous on the same generated line.
        let line_runs = match self.by_dst_line.get(lo_line as usize) {
            Some(r) => r,
            None => return false,
        };
        // Positions of lo and hi within the line's sorted run list.
        let lo_pos = match line_runs.iter().position(|&i| i == lo.0) {
            Some(p) => p,
            None => return false,
        };
        let hi_pos = match line_runs.iter().position(|&i| i == hi.0) {
            Some(p) => p,
            None => return false, // different generated line -> not compatible
        };
        for w in line_runs[lo_pos..=hi_pos].windows(2) {
            let left = &self.runs[w[0] as usize];
            let right = &self.runs[w[1] as usize];
            if left.dst_end != right.dst_col {
                return false; // a gap / synthetic content between the runs
            }
        }
        true
    }

    /// Get the underlying source map (for advanced queries).
    pub fn source_map(&self) -> &OwnedSourceMap {
        &self.map
    }
}

/// UTF-16 code-unit length of a single line (the slice already excludes the `\n`). A trailing
/// `\r` (CRLF) is counted as one unit, matching how an LSP `LineIndex` measures columns.
#[inline]
fn utf16_line_len(line: &str) -> u32 {
    line.chars().map(|c| c.len_utf16() as u32).sum()
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
    // Within-run delta bounded by the token's TRUE extent (not EOL, not "to the
    // next MAPPED token"). A query in a GAP between two mapped tokens, or past the
    // real content end of the last token on a line, returns None.
    // ========================================================================

    /// `vue_to_tsx`: a source query in a GAP between two mapped tokens — where the first
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
            mapper.vue_to_tsx(vue(0, 12)).is_none(),
            "source query in an inter-token gap must not snap to the preceding run: {:?}",
            mapper.vue_to_tsx(vue(0, 12))
        );
        // The first run's true interior (col 2, inside [0,3)) still maps.
        assert_eq!(
            mapper.vue_to_tsx(vue(0, 2)).unwrap().pos,
            TsPosition::new(0, 2)
        );
        // The last in-run col of the first run (2) maps; col 3 is the boundary -> None.
        assert!(mapper.vue_to_tsx(vue(0, 3)).is_none());
        // The second run still maps at its start.
        assert_eq!(
            mapper.vue_to_tsx(vue(0, 20)).unwrap().pos,
            TsPosition::new(0, 20)
        );
    }

    /// `tsx_to_vue`: a query past the real content end of the LAST mapped token on a line
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
            mapper.tsx_to_vue(ts(0, 0)).unwrap().pos,
            LspPosition::new(0, 0)
        );
        assert_eq!(
            mapper.tsx_to_vue(ts(0, 1)).unwrap().pos,
            LspPosition::new(0, 1)
        );
        // Col 2 is one-past the real content end -> None (not extended to EOL).
        assert!(
            mapper.tsx_to_vue(ts(0, 2)).is_none(),
            "query at the content end of the last run must be None: {:?}",
            mapper.tsx_to_vue(ts(0, 2))
        );
        // Col 5 (well past content) -> None.
        assert!(mapper.tsx_to_vue(ts(0, 5)).is_none());
    }

    /// Symmetric: `vue_to_tsx` past the real content end of the last mapped token on a
    /// source line must be `None` (true source-line-length bound, not EOL).
    #[test]
    fn test_vue_to_tsx_past_last_token_content_returns_none() {
        // Source line 0 is "ab" (length 2). gen(0,5)->src(0,0); last on both lines.
        let json = build_test_source_map("App.vue", "ab", &[(0, 5, 0, 0)]);
        let mapper = PositionMapper::from_json(&json).unwrap();

        assert_eq!(
            mapper.vue_to_tsx(vue(0, 1)).unwrap().pos,
            TsPosition::new(0, 6)
        );
        assert!(
            mapper.vue_to_tsx(vue(0, 2)).is_none(),
            "source query past content end must be None: {:?}",
            mapper.vue_to_tsx(vue(0, 2))
        );
    }

    /// Over-correction guard: a legitimate multiline mapped expression — where the queried
    /// columns ARE within the real source content of each line — still maps on every line.
    #[test]
    fn test_multiline_mapped_expression_in_bounds_still_maps() {
        // Source lines 4, 5, 6 each hold real 12-char content "const v = 1;"; gen line i
        // maps to src line 4+i. (Lines 0-3 are filler so the indices match a realistic SFC.)
        let src = "\n\n\n\nconst v = 1;\nconst v = 1;\nconst v = 1;\n";
        let json =
            build_test_source_map("App.vue", src, &[(0, 0, 4, 0), (1, 0, 5, 0), (2, 0, 6, 0)]);
        let mapper = PositionMapper::from_json(&json).unwrap();

        // Cols 2/3/4 are all within the 12-char source lines.
        assert_eq!(
            mapper.tsx_to_vue(ts(0, 2)).unwrap().pos,
            LspPosition::new(4, 2)
        );
        assert_eq!(
            mapper.tsx_to_vue(ts(1, 3)).unwrap().pos,
            LspPosition::new(5, 3)
        );
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
            mapper.tsx_to_vue(ts(0, 1)).is_some(),
            "precondition: start endpoint maps"
        );
        assert!(
            mapper.tsx_to_vue(ts(0, 9)).is_some(),
            "precondition: end endpoint maps"
        );
        assert!(
            mapper.tsx_range_to_vue(ts(0, 1), ts(0, 9)).is_none(),
            "a range straddling synthetic content between two runs must be dropped: {:?}",
            mapper.tsx_range_to_vue(ts(0, 1), ts(0, 9))
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
        let (start, end) = mapper.tsx_range_to_vue(ts(0, 1), ts(0, 4)).unwrap();
        assert_eq!(start, LspPosition::new(0, 11));
        assert_eq!(end, LspPosition::new(0, 14));

        // Half-open end exactly at the run's exclusive end (gen col 6 == run end) maps to
        // the run's mapped source end (src col 16).
        let (start, end) = mapper.tsx_range_to_vue(ts(0, 0), ts(0, 6)).unwrap();
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
        let (start, end) = mapper.tsx_range_to_vue(ts(0, 1), ts(0, 5)).unwrap();
        assert_eq!(start, LspPosition::new(0, 1));
        assert_eq!(end, LspPosition::new(0, 5));
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
