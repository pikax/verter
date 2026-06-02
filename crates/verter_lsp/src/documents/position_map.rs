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
///
/// Each run also carries a `component_id`: a compatibility-component label assigned at
/// construction (see [`PositionMapper::precompute_runs`]). Two runs may compose a range iff
/// they share a `component_id` — i.e. they are linked by an unbroken chain of runs that are
/// contiguous in BOTH the generated and the source space (same-line adjacency, or the
/// multiline line-wrap equivalent). This is what makes [`PositionMapper::runs_compatible`]
/// an O(1) field comparison AND bakes source-contiguity into compatibility, so two
/// generated-adjacent but source-discontiguous runs (the `MoveOriginal` / repeated-emission
/// shape) never compose a bogus straddling source range.
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
    /// Compatibility-component label: runs with the same id are linked by an unbroken chain
    /// of both-space-contiguous runs and may compose a range; runs with different ids may not.
    component_id: u32,
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
/// requires both endpoints to resolve inside the SAME **compatibility component** (the same
/// run, or runs linked by an unbroken chain that is contiguous in BOTH the generated and the
/// source space — same-line adjacency or the multiline line-wrap equivalent). Two endpoints
/// that each map but lie in runs separated by synthetic content, or in generated-adjacent
/// but source-discontiguous runs (the `MoveOriginal` / repeated-emission shape), do NOT
/// compose — there is no bogus straddling source range.
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
    /// A run's stable identity is its index into this vec; each run also carries a
    /// `component_id` (assigned in `precompute_runs`) so endpoint-compatibility is an O(1)
    /// label comparison rather than a per-call chain walk.
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
        // Compatibility-component labelling. Runs are produced in source-map token order
        // (sorted by `(dst_line, dst_col)`), so we can assign component ids in this same pass:
        // a run joins the previous run's component iff it is contiguous with it in BOTH spaces
        // (`runs_both_space_contiguous`); otherwise it starts a fresh component.
        let mut next_component: u32 = 0;
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
            // synthetic suffix or to EOL. It is ABSENT only when the map carries no embedded
            // `sourcesContent`: an INTERIOR run is then bounded exactly by `next_dst_bound`,
            // which — by the position-preserving invariant `src_end - src_col == dst_end -
            // dst_col` — is the TRUE source extent, not a permissive guess; a LAST-on-line run
            // has no extent signal and is conservatively dropped below (`run_len == 0`).
            let src_col = token.get_src_col();
            let src_remaining = src_line_len(source_id, token.get_src_line())
                .map(|len| len.saturating_sub(src_col));

            let run_len = match (next_dst_bound, src_remaining) {
                (Some(a), Some(b)) => a.min(b),
                // No source-line length available (no embedded source content): by the
                // position-preserving invariant an interior run's source extent equals its
                // generated extent, so `next_dst_bound` IS the true source extent — provably
                // exact, never permissive.
                (Some(a), None) => a,
                (None, Some(b)) => b,
                // No following token and no source-line info: a last-on-line run with no
                // content has no extent signal at all. Drop it (covers no columns) rather than
                // extend it to EOL — the conservative, non-permissive choice.
                (None, None) => 0,
            };

            if run_len == 0 {
                continue;
            }

            let run = MappedRun {
                dst_line,
                dst_col,
                dst_end: dst_col + run_len,
                src_line: token.get_src_line(),
                src_col,
                src_end: src_col + run_len,
                component_id: 0, // assigned just below
            };

            // Start a new component unless this run continues the previous run in BOTH spaces.
            let component_id = match runs.last() {
                Some(prev) if Self::runs_both_space_contiguous(prev, &run) => prev.component_id,
                _ => {
                    let id = next_component;
                    next_component += 1;
                    id
                }
            };
            runs.push(MappedRun {
                component_id,
                ..run
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
    /// A range maps only when both endpoints resolve inside the SAME compatibility component:
    ///  - the `start` position resolves inside some run `S`; and
    ///  - the half-open `end` either (a) equals `S`'s exclusive generated end exactly (the
    ///    range terminates at `S`'s mapped end — same run), or (b) resolves inside a run `E`
    ///    that shares `S`'s `component_id` (linked to `S` by an unbroken chain that is
    ///    contiguous in BOTH the generated and the source space — see
    ///    [`Self::runs_both_space_contiguous`]).
    ///
    /// Any range whose endpoints fall in runs separated by synthetic/unmapped content, on
    /// different lines, in generated-adjacent but source-discontiguous runs (the
    /// `MoveOriginal` / repeated-emission shape), or otherwise in different components, returns
    /// `None` — never a bogus straddling range. This is the binding "both endpoints inside
    /// compatible mapped spans" contract.
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

    /// Whether `cur` directly continues `prev` in BOTH the generated and the source space —
    /// the construction-time predicate that joins two runs into one compatibility component.
    ///
    /// Generated-side contiguity alone is NOT enough: the generated output relocates and
    /// repeats source (`MoveOriginal`; a native v-model expression emitted several times), so
    /// two generated-adjacent runs can map to reordered / repeated / non-adjacent source.
    /// Requiring source-side contiguity too is what guarantees a well-formed SOURCE range.
    ///
    /// Two shapes are contiguous:
    ///  - **Same-line**: `prev.dst_end == cur.dst_col` on one generated line AND
    ///    `prev.src_end == cur.src_col` on one source line (no synthetic/unmapped content and
    ///    no source reorder between them).
    ///  - **Line-wrap** (a legitimate multiline mapped expression): `prev` ends at the EOL of
    ///    generated line `N` and `cur` starts at column 0 of generated line `N+1`, and the
    ///    source side wraps the same way (`cur.src_line == prev.src_line + 1 && cur.src_col ==
    ///    0`). Because runs arrive in `(dst_line, dst_col)` order, an immediately-following run
    ///    one generated line down at column 0 means `prev` was the last run on its line — so
    ///    this is exactly the position-preserving wrap, not a reorder.
    ///
    /// Anything else (a gap, a reorder, a repeat, a different source) is a genuine
    /// discontinuity and starts a new component.
    fn runs_both_space_contiguous(prev: &MappedRun, cur: &MappedRun) -> bool {
        let same_line = prev.dst_line == cur.dst_line
            && prev.dst_end == cur.dst_col
            && prev.src_line == cur.src_line
            && prev.src_end == cur.src_col;
        let line_wrap = cur.dst_line == prev.dst_line + 1
            && cur.dst_col == 0
            && cur.src_line == prev.src_line + 1
            && cur.src_col == 0;
        same_line || line_wrap
    }

    /// Whether two mapped runs are COMPATIBLE for composing a range. O(1): two runs may
    /// compose iff they share a compatibility component — i.e. they are linked by an unbroken
    /// chain of runs that are contiguous in BOTH spaces (see
    /// [`Self::runs_both_space_contiguous`] and the labelling pass in
    /// [`Self::precompute_runs`]). The same run is trivially compatible with itself.
    ///
    /// Because the component label already encodes source-contiguity, this catches the
    /// generated-adjacent-but-source-discontiguous case (`MoveOriginal` / repeated emission)
    /// that a generated-only adjacency check would wrongly accept — without any per-call
    /// linear scan or chain walk.
    fn runs_compatible(&self, a: RunId, b: RunId) -> bool {
        self.runs[a.0 as usize].component_id == self.runs[b.0 as usize].component_id
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
#[path = "position_map_tests.rs"]
mod position_map_tests;
