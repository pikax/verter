use oxc_sourcemap::OwnedSourceMap;
use verter_span::{LspPosition, TsPosition};

/// A single mapped run, precomputed once at [`PositionMapper`] construction.
///
/// A run is one mapped source-map token's contiguous mapped extent. The `.vue` eval source is
/// **position-preserving** (`IndexedReady.eval_source`), so the generated and source extents
/// of a run have the SAME length by construction: `dst_end - dst_col == src_end - src_col`.
/// That position-preserving assumption is only *relied upon* (to infer a source extent the
/// source map does not spell out) when the map carries no embedded source content; in that
/// case it is verified mechanically against the token stream before being trusted (see the
/// content-less inference arm + [`PositionMapper::tokens_are_position_preserving`]), and an
/// independent `debug_assert!` pins it for the content-present interior case.
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
    /// The source this run maps into (`Token::get_source_id`). Two runs are contiguous only
    /// when they share a `source_id`: matching line/col geometry across two DIFFERENT sources
    /// cannot compose a single source range (an `LspPosition` carries no source identity), so
    /// the contiguity predicate must reject it.
    source_id: u32,
    /// Whether the token that produced this run is the LAST token of ANY kind on its generated
    /// line (its `next_dst_bound` is `None`). A run that is NOT last-on-line has synthetic/
    /// unmapped generated content after it, so it must not line-wrap-join the next-line run.
    last_on_dst_line: bool,
    /// Whether this run's source extent reaches its source line's true content end
    /// (`src_end == source-line length`). Required for a line-wrap join so the source side is
    /// genuinely contiguous across the newline (nothing between the run's source end and EOL).
    /// `false` when the map carries no source content (the source-line length is unknown, so a
    /// source-contiguous wrap cannot be proven and the join is conservatively rejected).
    src_reaches_line_end: bool,
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
    ///
    /// The per-token "next generated bound" is computed in a SINGLE backward pass
    /// ([`Self::next_dst_bounds`]) rather than a forward scan per token, so construction stays
    /// O(n) in the token count — not O(n²) on a long generated line.
    #[allow(clippy::type_complexity)]
    fn precompute_runs(map: &OwnedSourceMap) -> (Vec<MappedRun>, Vec<Vec<u32>>, Vec<Vec<u32>>) {
        let tokens: Vec<_> = map.get_tokens().collect();

        // The next-token-on-the-same-generated-line column for every token, precomputed in one
        // O(n) backward pass (not an O(n²) forward scan per token). `None` means the token is
        // the LAST token of ANY kind on its generated line.
        let next_dst_bounds = Self::next_dst_bounds(&tokens);

        // Whether the WHOLE map is position-preserving (the source column advances in lockstep
        // with the generated column). When the map carries NO embedded source content, an
        // interior run's source extent is inferred from `next_dst_bound` via this invariant; if
        // the map is NOT position-preserving that inference is unsound, so the content-less
        // inference arm is taken only when this holds (else the run is dropped). Derived from an
        // INDEPENDENT comparison of consecutive same-line tokens' source-vs-generated deltas —
        // not a tautology over a single run's own (equal-by-construction) extents.
        let position_preserving = Self::tokens_are_position_preserving(&tokens);

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

            // Bound 1: the next token of ANY kind on the same generated line (precomputed in
            // one backward pass). A gap to the next mapped token is therefore NOT swallowed (an
            // unmapped token in between, or the next mapped token itself, caps this run at its
            // own start). `None` => this token is the last of any kind on its generated line.
            let next_dst_token_col = next_dst_bounds[idx];
            let next_dst_bound = next_dst_token_col.map(|c| c - dst_col);
            let last_on_dst_line = next_dst_token_col.is_none();

            // Bound 2: the token's own source line's true content length remaining. This
            // caps a last-on-line run so it cannot extend past real source text into a
            // synthetic suffix or to EOL. It is ABSENT only when the map carries no embedded
            // `sourcesContent`: an INTERIOR run is then bounded exactly by `next_dst_bound`,
            // which — by the position-preserving invariant `src_end - src_col == dst_end -
            // dst_col` — is the TRUE source extent (taken ONLY when the map is provably
            // position-preserving); a LAST-on-line run has no extent signal and is
            // conservatively dropped below (`run_len == 0`).
            let src_line = token.get_src_line();
            let src_col = token.get_src_col();
            let src_line_total = src_line_len(source_id, src_line);
            let src_remaining = src_line_total.map(|len| len.saturating_sub(src_col));

            let run_len = match (next_dst_bound, src_remaining) {
                // Source content present: the source extent is observed DIRECTLY as `b` and
                // clamps the generated reach `a` (so a far-away sentinel/synthetic next token
                // cannot over-extend the run past the real source text). No position-preserving
                // inference is needed here — the source line is ground truth — so this arm is
                // robust to non-position-preserving maps (`MoveOriginal`, reorder, repeat).
                (Some(a), Some(b)) => a.min(b),
                // No source-line length available (no embedded source content): take the
                // position-preserving inference (`next_dst_bound` IS the source extent) ONLY
                // when the map is provably position-preserving — the mechanical provenance gate.
                // Otherwise the inference is unsound and the run is dropped: a malformed,
                // non-lockstep content-less map yields `None`, never a fabricated source extent.
                (Some(a), None) if position_preserving => a,
                (Some(_), None) => 0,
                (None, Some(b)) => b,
                // No following token and no source-line info: a last-on-line run with no
                // content has no extent signal at all. Drop it (covers no columns) rather than
                // extend it to EOL — the conservative, non-permissive choice.
                (None, None) => 0,
            };

            if run_len == 0 {
                continue;
            }

            let src_end = src_col + run_len;
            // Whether this run reaches its source line's true content end. Used by the
            // line-wrap join so the source side is contiguous across the newline. Unknown
            // without source content => `false` => a wrap cannot be proven => no join.
            let src_reaches_line_end = src_line_total == Some(src_end);

            let run = MappedRun {
                dst_line,
                dst_col,
                dst_end: dst_col + run_len,
                src_line,
                src_col,
                src_end,
                source_id,
                last_on_dst_line,
                src_reaches_line_end,
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

    /// For each token (by index), the `dst_col` of the next token of ANY kind on the SAME
    /// generated line whose column is STRICTLY greater — or `None` when the token is the last
    /// of any kind on its generated line.
    ///
    /// Computed in a SINGLE backward pass: tokens arrive sorted by `(dst_line,
    /// dst_col)`, so the next strictly-greater-column boundary of token `i` is either
    /// token `i+1`'s column (when it is on the same line and strictly greater) or — when
    /// `i+1` shares `i`'s column — the SAME boundary already computed for `i+1`. This
    /// replicates the previous forward `take_while(...).find(c > dst_col)` scan exactly while
    /// being O(n) overall instead of O(n²) on a long generated line.
    fn next_dst_bounds(tokens: &[oxc_sourcemap::Token]) -> Vec<Option<u32>> {
        let n = tokens.len();
        let mut bounds = vec![None; n];
        if n == 0 {
            return bounds;
        }
        // The last token overall is the last on its line -> `None` (already the default).
        for idx in (0..n - 1).rev() {
            let cur = &tokens[idx];
            let next = &tokens[idx + 1];
            bounds[idx] = if next.get_dst_line() != cur.get_dst_line() {
                // `next` is on a later generated line: `cur` is the last token on its line.
                None
            } else if next.get_dst_col() > cur.get_dst_col() {
                // Immediate next token on the same line with a strictly greater column.
                Some(next.get_dst_col())
            } else {
                // `next` shares `cur`'s column (a co-located token): the first strictly-greater
                // column after `cur` is the same as the one already computed for `next`.
                bounds[idx + 1]
            };
        }
        bounds
    }

    /// Whether the token stream is position-preserving: the source column advances in LOCKSTEP
    /// with the generated column between consecutive mapped tokens that share a generated line
    /// AND a source line AND a source id. For every such adjacent pair the source-column delta
    /// must equal the generated-column delta.
    ///
    /// This is an INDEPENDENT check (it compares two distinct tokens' source-vs-generated
    /// deltas — never a single run's own equal-by-construction extents), and it gates the
    /// content-less source-extent inference: when the map carries no source content the
    /// inferred extent is sound only if this holds. A pair that violates lockstep makes the
    /// whole map non-position-preserving, so the content-less inference is rejected outright
    /// (the conservative, non-permissive choice) rather than fabricating a wrong source extent.
    fn tokens_are_position_preserving(tokens: &[oxc_sourcemap::Token]) -> bool {
        for pair in tokens.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            let (Some(sa), Some(sb)) = (a.get_source_id(), b.get_source_id()) else {
                continue; // an unmapped token breaks adjacency; no lockstep claim across it
            };
            if sa != sb
                || a.get_dst_line() != b.get_dst_line()
                || a.get_src_line() != b.get_src_line()
            {
                continue; // different gen line / src line / source: not a same-line lockstep pair
            }
            // Both deltas are non-negative (tokens sorted by dst_col; same-source same-line).
            let dst_delta = b.get_dst_col().saturating_sub(a.get_dst_col());
            let src_delta = b.get_src_col().saturating_sub(a.get_src_col());
            if dst_delta != src_delta {
                return false;
            }
        }
        true
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

        // Half-open end at the EXCLUSIVE generated end of a run: the last included column is
        // `end.character - 1`, which lies inside the terminal run. That run may be
        // the start run itself OR any LATER run in the same compatibility component (a
        // multi-run range like `[1,6)` over `[0,3)+[3,6)`, whose end equals the SECOND run's
        // exclusive end). Resolve the run containing the last included column and accept it
        // when its exclusive end is exactly `end` and it is compatible with the start run; the
        // composed Vue end is that run's mapped exclusive end. (Subsumes the previous
        // start-run-only case: when `end` is the start run's own exclusive end, `end - 1` is
        // inside the start run.)
        if end.character > 0 {
            if let Some(end_run_id) = self.run_at_dst(end.line, end.character - 1) {
                let end_run = &self.runs[end_run_id.0 as usize];
                if end_run.dst_end == end.character
                    && self.runs_compatible(start_mapped.run, end_run_id)
                {
                    return Some((
                        start_mapped.pos,
                        LspPosition {
                            line: end_run.src_line,
                            character: end_run.src_end,
                        },
                    ));
                }
            }
        }

        // Otherwise the end must resolve INSIDE a run compatible with the start run.
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
    /// Both shapes additionally require the SAME `source_id`: matching line/col geometry across
    /// two DIFFERENT sources cannot compose one source range (an `LspPosition` carries no source
    /// identity), so it must NOT join.
    ///
    /// Two shapes are contiguous:
    ///  - **Same-line**: `prev.dst_end == cur.dst_col` on one generated line AND
    ///    `prev.src_end == cur.src_col` on one source line (no synthetic/unmapped content and
    ///    no source reorder between them).
    ///  - **Line-wrap** (a legitimate multiline mapped expression): there is genuinely NOTHING
    ///    between `prev` and `cur` across the newline. This requires (a) `prev` is the LAST
    ///    token of any kind on its generated line (`prev.last_on_dst_line` — a synthetic/
    ///    unmapped tail would otherwise sit after it), (b) `prev` reaches its source line's true
    ///    content end (`prev.src_reaches_line_end` — nothing between its source end
    ///    and EOL; also `false` when source content is absent, so the wrap cannot be proven and
    ///    is rejected), and (c) `cur` starts at column 0 of the next generated line AND the next
    ///    source line (`cur.dst_line == prev.dst_line + 1 && cur.dst_col == 0 && cur.src_line ==
    ///    prev.src_line + 1 && cur.src_col == 0`). Only then is the generated newline the sole
    ///    content between the two runs in BOTH spaces — the position-preserving wrap, not a
    ///    reorder and not a bridge over a synthetic tail.
    ///
    /// Anything else (a gap, a synthetic tail, a reorder, a repeat, a different source) is a
    /// genuine discontinuity and starts a new component.
    fn runs_both_space_contiguous(prev: &MappedRun, cur: &MappedRun) -> bool {
        if prev.source_id != cur.source_id {
            return false;
        }
        let same_line = prev.dst_line == cur.dst_line
            && prev.dst_end == cur.dst_col
            && prev.src_line == cur.src_line
            && prev.src_end == cur.src_col;
        let line_wrap = prev.last_on_dst_line
            && prev.src_reaches_line_end
            && cur.dst_line == prev.dst_line + 1
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
