//! Architecture guard: `PositionMapper` is a STRICT in-run coordinate mapper.
//!
//! CRITICAL rule (see `CLAUDE.md` Fallthrough/position substrate + the goto-definition
//! plan PHASE 1): the two public mapper methods take/return TYPED coordinates
//! (`TsPosition` / `LspPosition`, never raw `(line, column)` tuples) and return `Some`
//! ONLY when the query lies strictly inside ONE mapped token's run. There is NO
//! cross-token extrapolation and NO snap-to-closest-preceding fallback; a query in
//! unmapped/synthetic content, in a gap, or bridging into the next token maps to `None`.
//! Within-run character precision IS preserved (the in-run offset is added to the run's
//! mapped start), but only inside a single mapped run.
//!
//! This file pins the rule two ways:
//!   1. A behavioural test on a constructed mapper (the typed signature + the
//!      in-run/None contract), which discriminates BOTH directions of regression:
//!      a re-introduced cross-token fallback makes the unmapped-interior query `Some`
//!      (fails), and deleting within-run precision makes the in-run query `None`/wrong
//!      (fails).
//!   2. A static source scan (`ban_cross_token_extrapolation`) asserting the deleted
//!      extrapolation/snap markers are ABSENT from `position_map.rs` — re-introducing
//!      `tsx_to_vue`'s `best_dst_col` nearest-previous loop or `vue_to_tsx`'s
//!      `best_src_col`/`best_src_line` "highest src <= target" snap loop fails the scan.

use std::path::PathBuf;

use verter_lsp::documents::position_map::PositionMapper;
use verter_span::{LspPosition, TsPosition};

/// Build a source map with both mapped and unmapped tokens.
/// Mapped tokens: (dst_line, dst_col, src_line, src_col); unmapped: (dst_line, dst_col).
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
    // A re-introduced cross-token fallback (backward scan / snap) would make this Some.
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

/// Static scan: the deleted cross-token extrapolation / snap markers must be ABSENT from
/// `position_map.rs`. Scoped NOT to ban the within-run guarded delta (which is an
/// addition `+ (column - dst_col)` inside the in-run branch, not a `pos.column +=` mutation
/// and not a `best_*` accumulator).
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

    // Banned markers from the deleted extrapolation/snap fallbacks.
    const BANNED: &[(&str, &str)] = &[
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
        (
            "pos.column +=",
            "in-place cross-token column-delta mutation",
        ),
        (
            "result.column +=",
            "in-place cross-token column-delta mutation",
        ),
    ];
    for (marker, what) in BANNED {
        assert!(
            !prod.contains(marker),
            "position_map.rs production code must not contain `{marker}` ({what}); \
             the mapper is strict in-run only — no cross-token extrapolation/snap."
        );
    }

    // Positive anchors: the strict in-run helpers MUST be present (so the scan is pinned
    // to the actual implementation, not vacuously passing on a renamed-away body).
    for anchor in ["next_dst_col_on_line", "next_src_col_on_line"] {
        assert!(
            prod.contains(anchor),
            "position_map.rs must keep the in-run bound helper `{anchor}`"
        );
    }
}
