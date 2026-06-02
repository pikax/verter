//! Architecture guard: `PositionMapper` is a STRICT in-run coordinate mapper.
//!
//! CRITICAL rule (see `CLAUDE.md` Fallthrough/position substrate + the goto-definition
//! plan + the architecture decision "A. MAPPING SUBSTRATE"): the two public mapper
//! methods take/return TYPED coordinates (`TsPosition` / `LspPosition`, never raw
//! `(line, column)` tuples) and return `Some` ONLY when the query lies strictly inside ONE
//! mapped run. There is NO cross-token extrapolation and NO snap-to-closest-preceding
//! fallback; a query in unmapped/synthetic content, in a gap between runs, past a run's true
//! content end, or bridging into the next run maps to `None`. Within-run character precision
//! IS preserved (the in-run offset is added to the run's mapped start), but only inside a
//! single mapped run.
//!
//! A RANGE maps only via `tsx_range_to_vue`, which requires both endpoints to resolve inside
//! COMPATIBLE runs (the same run, or genuinely-contiguous runs with no synthetic/unmapped
//! content between them). Two endpoints that each map but lie in runs separated by synthetic
//! content do NOT compose — the range is dropped, never a bogus straddling range.
//!
//! This file pins the rule two ways:
//!   1. Behavioural tests on a constructed mapper (the typed signature + the in-run/None
//!      contract + the range-compatibility contract), each of which discriminates a concrete
//!      regression — a re-introduced cross-token fallback, a deleted within-run delta, an
//!      EOL-overextended last run, an inter-run gap snap, or a cross-run range leak.
//!   2. A static source scan (`ban_cross_token_extrapolation`) asserting the deleted
//!      extrapolation/snap markers — and the GENERAL delta-mutation-outside-the-guard
//!      pattern — are ABSENT from `position_map.rs`.

use std::path::PathBuf;

use verter_lsp::documents::position_map::PositionMapper;
use verter_span::{LspPosition, TsPosition};

/// Build a source map with both mapped and unmapped tokens.
/// Mapped tokens: (dst_line, dst_col, src_line, src_col); unmapped: (dst_line, dst_col).
#[allow(clippy::type_complexity)] // compact (line, col, Option<(src_line, src_col)>) token tuple
fn build_map(
    mapped: &[(u32, u32, u32, u32)],
    unmapped: &[(u32, u32)],
    source: &str,
) -> PositionMapper {
    let mut builder = oxc_sourcemap::SourceMapBuilder::default();
    let source_id = builder.set_source_and_content("Guard.vue", source);

    let mut all: Vec<(u32, u32, Option<(u32, u32)>)> = Vec::new();
    for &(dl, dc, sl, sc) in mapped {
        all.push((dl, dc, Some((sl, sc))));
    }
    for &(dl, dc) in unmapped {
        all.push((dl, dc, None));
    }
    all.sort_by_key(|(l, c, _)| (*l, *c));
    for (dl, dc, src) in all {
        match src {
            Some((sl, sc)) => builder.add_token(dl, dc, sl, sc, Some(source_id), None),
            None => builder.add_token(dl, dc, 0, 0, None, None),
        };
    }
    PositionMapper::from_json(&builder.into_sourcemap().to_json_string()).unwrap()
}

#[test]
fn mapper_methods_take_typed_coordinates_and_return_option() {
    // ── A single UNMAPPED token: an interior query must be None. ──
    let only_unmapped = build_map(&[], &[(0, 0)], "x");
    assert!(
        only_unmapped.tsx_to_vue(TsPosition::new(0, 3)).is_none(),
        "interior of an unmapped-only map must be None (no cross-token fallback)"
    );
    assert!(
        only_unmapped.vue_to_tsx(LspPosition::new(0, 3)).is_none(),
        "vue_to_tsx over an unmapped-only map must be None"
    );

    // ── A single MAPPED multi-char run: in-run query must be Some with the correct
    //    within-run column. Deleting within-run precision would make this None/wrong. ──
    // gen(0,0) -> src(0,10); an unmapped token at gen(0,6) bounds the run to [0,6).
    let mapped = build_map(&[(0, 0, 0, 10)], &[(0, 6)], &" ".repeat(40));
    let m = mapped
        .tsx_to_vue(TsPosition::new(0, 3))
        .expect("in-run query must map (within-run precision preserved)");
    // Typed return: a TSX position in -> an LSP (Vue) position out.
    assert_eq!(m.pos, LspPosition::new(0, 13));

    // Bridging past the run end (onto the unmapped boundary) must be None.
    assert!(
        mapped.tsx_to_vue(TsPosition::new(0, 6)).is_none(),
        "query at the unmapped run boundary must be None (no bridge into next token)"
    );

    // Symmetric vue_to_tsx within-run precision + None outside the source run.
    let src_run = build_map(&[(0, 10, 0, 0), (0, 40, 0, 6)], &[], &" ".repeat(40));
    let g = src_run
        .vue_to_tsx(LspPosition::new(0, 3))
        .expect("in-run source query must map");
    // src run [0,6) -> gen base 10: src 3 -> gen 13.
    assert_eq!(g.pos, TsPosition::new(0, 13));
    // A source line with no mapped token must be None (no snap to a preceding line).
    assert!(
        src_run.vue_to_tsx(LspPosition::new(5, 0)).is_none(),
        "unmapped source line must be None (no snap-to-closest-preceding)"
    );
}

/// Primary behavioural guard: each assertion discriminates a SPECIFIC re-introduced defect,
/// and the setups are chosen so a revival of the corresponding non-strict behaviour (a
/// backward-scan snap, a bound-by-next-mapped-token run, an extend-to-EOL run, or a
/// per-endpoint range composer) would FAIL them — so a regression trips this test.
#[test]
fn strict_in_run_behaviour_discriminates_each_deleted_path() {
    // ── (a) MAPPED-token-BEFORE-unmapped-interior: a backward-scan snap would have snapped
    //    this interior query back to the preceding MAPPED token and returned
    //    Some(extrapolated). The strict mapper returns None because the covering token is
    //    unmapped. (Discriminating where the unmapped-ONLY case is NOT — there a backward
    //    scan finds no mapped token and also returns None.) ──
    // mapped gen(0,0)->src(0,0) run [0,3); unmapped gen(0,3) (synthetic) follows.
    let m = build_map(&[(0, 0, 0, 0)], &[(0, 3)], &" ".repeat(40));
    assert!(
        m.tsx_to_vue(TsPosition::new(0, 5)).is_none(),
        "interior of an unmapped token that FOLLOWS a mapped token must be None — a revived \
         backward-scan would snap to the mapped token and return Some"
    );
    // The preceding mapped run still maps within its true extent [0,3).
    assert_eq!(
        m.tsx_to_vue(TsPosition::new(0, 2)).unwrap().pos,
        LspPosition::new(0, 2)
    );

    // ── (b) Inter-token GAP: a source query between two mapped tokens, past the first run's
    //    TRUE extent, must be None — a "bound by next MAPPED token" run would let the first
    //    run span the gap and snap the query into it. ──
    // gen(0,0)->src(0,0) bounded by unmapped gen(0,3); next mapped gen(0,20)->src(0,20).
    let gap = build_map(&[(0, 0, 0, 0), (0, 20, 0, 20)], &[(0, 3)], &" ".repeat(40));
    assert!(
        gap.vue_to_tsx(LspPosition::new(0, 12)).is_none(),
        "source query in an inter-token gap must be None (no snap into the preceding run)"
    );

    // ── (c) Last-run EOL over-extension: a query past the real content end of the last
    //    mapped token on a line must be None — an "extend to EOL" run would map it. ──
    // Source line "ab" (len 2); single mapped run gen(0,0)->src(0,0) bounded to [0,2).
    let last = build_map(&[(0, 0, 0, 0)], &[], "ab");
    assert_eq!(
        last.tsx_to_vue(TsPosition::new(0, 1)).unwrap().pos,
        LspPosition::new(0, 1)
    );
    assert!(
        last.tsx_to_vue(TsPosition::new(0, 5)).is_none(),
        "query past the last run's true content end must be None (not extended to EOL)"
    );

    // ── (d) Range endpoint COMPATIBILITY: a range whose endpoints fall in two runs
    //    separated by synthetic content must be DROPPED — a per-endpoint composer (each
    //    endpoint maps independently) would return a bogus straddling range. ──
    // run A gen(0,0)->src(0,0) [0,3); synthetic gen(0,3); run B gen(0,8)->src(0,50).
    let ranges = build_map(&[(0, 0, 0, 0), (0, 8, 0, 50)], &[(0, 3)], &" ".repeat(80));
    assert!(
        ranges.tsx_to_vue(TsPosition::new(0, 1)).is_some()
            && ranges.tsx_to_vue(TsPosition::new(0, 9)).is_some(),
        "precondition: both endpoints individually map"
    );
    assert!(
        ranges
            .tsx_range_to_vue(TsPosition::new(0, 1), TsPosition::new(0, 9))
            .is_none(),
        "a range straddling synthetic content between two runs must be dropped"
    );
    // A range fully inside run A maps.
    let (s, e) = ranges
        .tsx_range_to_vue(TsPosition::new(0, 1), TsPosition::new(0, 3))
        .expect("range inside one run must map");
    assert_eq!(s, LspPosition::new(0, 1));
    assert_eq!(e, LspPosition::new(0, 3));
}

/// Static scan: the deleted cross-token extrapolation / snap markers — and the GENERAL
/// delta-mutation-outside-the-within-run-guard pattern — must be ABSENT from
/// `position_map.rs`. Scoped NOT to ban the within-run guarded delta (an addition
/// `+ (column - run.dst_col)` / `+ (column - run.src_col)` inside the in-run branch — an
/// expression, not an in-place mutation, and not a `best_*` accumulator).
#[test]
fn ban_cross_token_extrapolation() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("documents")
        .join("position_map.rs");
    let src =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));

    // Restrict the scan to the production code (exclude the `#[cfg(test)]` module, whose
    // doc-comments and probes legitimately *describe* the banned behaviour they delete).
    let prod = match src.find("#[cfg(test)]") {
        Some(idx) => &src[..idx],
        None => &src[..],
    };

    // (1) Exact-name markers from the deleted extrapolation/snap fallbacks — kept for a
    //     precise message if someone reintroduces the original code verbatim.
    const BANNED_EXACT: &[(&str, &str)] = &[
        (
            "best_dst_col",
            "tsx_to_vue nearest-previous backward-scan accumulator",
        ),
        (
            "best_src_col",
            "vue_to_tsx snap-to-closest-preceding accumulator",
        ),
        (
            "best_src_line",
            "vue_to_tsx snap-to-closest-preceding accumulator",
        ),
    ];
    for (marker, what) in BANNED_EXACT {
        assert!(
            !prod.contains(marker),
            "position_map.rs production code must not contain `{marker}` ({what}); \
             the mapper is strict in-run only — no cross-token extrapolation/snap."
        );
    }

    // (2) GENERAL delta-mutation pattern: any in-place column/character `+=` accumulation
    //     is forbidden in production. The strict within-run delta is a pure expression
    //     (`run.src_col + (column - run.dst_col)`), never a mutation, so banning `+=` onto a
    //     position/column/character field catches a RENAMED accumulator too (closing the
    //     "a renamed accumulator or `*.character +=` would slip" gap). Whitespace between the
    //     lhs and `+=` is normalised so `pos . character  +=` cannot evade it.
    let normalized: String = {
        // Collapse runs of ASCII whitespace to a single space so token-adjacency checks are
        // robust to formatting (`a  +=` / `a\n+=` -> `a +=`).
        let mut out = String::with_capacity(prod.len());
        let mut prev_ws = false;
        for ch in prod.chars() {
            if ch.is_ascii_whitespace() {
                if !prev_ws {
                    out.push(' ');
                }
                prev_ws = true;
            } else {
                out.push(ch);
                prev_ws = false;
            }
        }
        out
    };
    // NOTE: only `+=` accumulation is scanned (not bare `=` reassignment) because a `==`
    // comparison contains `=` as a substring and would false-positive. The in-place
    // delta-accumulation loops that were deleted all used `+=`; a reassignment-based snap is
    // additionally killed by the behavioural test (`strict_in_run_behaviour_*`), the primary
    // guard per the plan.
    const BANNED_MUTATION: &[(&str, &str)] = &[
        ("column +=", "in-place column-delta mutation"),
        ("character +=", "in-place character-delta mutation"),
        ("+= column", "column added into an accumulator"),
        ("+= (column", "column delta added into an accumulator"),
        ("+= character", "character added into an accumulator"),
    ];
    for (marker, what) in BANNED_MUTATION {
        assert!(
            !normalized.contains(marker),
            "position_map.rs production code must not contain `{marker}` ({what}); \
             the within-run delta is a pure expression, never an in-place position mutation \
             or accumulator — a re-introduced snap/extrapolation loop trips this scan."
        );
    }

    // Positive anchors: the strict in-run lookup + range-compatibility helpers MUST be
    // present (so the scan is pinned to the actual implementation, not vacuously passing on
    // a renamed-away body). These are the precompute + binary-search + compatibility entry
    // points that REPLACED the deleted scan/snap loops.
    for anchor in [
        "precompute_runs",
        "run_at_dst",
        "run_at_src",
        "runs_compatible",
    ] {
        assert!(
            prod.contains(anchor),
            "position_map.rs must keep the strict-mapper helper `{anchor}`"
        );
    }
}
