//! Unit tests for the projection-aware document → provider position mapping:
//! the line-only rewrite-aware [`super::SelfFileProviderMapper`] (prelude
//! offset, rewrite-region drop + column shift, range both-endpoint rule) and
//! the unified [`super::ProviderPositionMapper`] dispatch.

use super::*;
use crate::documents::line_index::LineIndex;
use tower_lsp_server::ls_types::PositionEncodingKind;

fn li(src: &str) -> LineIndex {
    LineIndex::new(src, PositionEncodingKind::UTF16)
}
#[test]
fn round_trips_source_provider_with_prelude_offset() {
    // 4-line prelude, no rewrites.
    let src = "export const s = $state(0);\nexport const t = s + 1;\n";
    let mapper = SelfFileProviderMapper::new(4, &[], &li(src));

    // source line 0 col 13 ('s' in `const s`) → provider line 4 col 13.
    let prov = mapper
        .carrier_to_tsx(LspPosition::new(0, 13))
        .expect("source maps to provider");
    assert_eq!(prov.pos, TsPosition::new(4, 13));

    // provider line 4 col 13 → source line 0 col 13.
    let back = mapper
        .tsx_to_carrier(TsPosition::new(4, 13))
        .expect("provider maps back to source");
    assert_eq!(back.pos, LspPosition::new(0, 13));

    // source line 1 → provider line 5.
    let prov = mapper
        .carrier_to_tsx(LspPosition::new(1, 7))
        .expect("line 1 maps");
    assert_eq!(prov.pos, TsPosition::new(5, 7));
}

#[test]
fn drops_prelude_region_never_clamps() {
    let src = "export const s = $state(0);\n";
    let mapper = SelfFileProviderMapper::new(4, &[], &li(src));

    // Any provider line inside the prelude (< 4) has NO user-source
    // correlation — dropped, NOT clamped to source line 0.
    for line in 0..4 {
        assert!(
            mapper.tsx_to_carrier(TsPosition::new(line, 0)).is_none(),
            "prelude line {line} must drop, not clamp"
        );
    }
    // The first NON-prelude line maps.
    assert!(mapper.tsx_to_carrier(TsPosition::new(4, 0)).is_some());
}

#[test]
fn range_maps_only_when_both_endpoints_in_user_region() {
    let src = "export const s = $state(0);\n";
    let mapper = SelfFileProviderMapper::new(4, &[], &li(src));

    // Both endpoints in the user region → maps.
    let mapped = mapper.tsx_range_to_carrier(TsPosition::new(4, 0), TsPosition::new(4, 5));
    assert_eq!(
        mapped,
        Some((LspPosition::new(0, 0), LspPosition::new(0, 5)))
    );

    // Start endpoint in the prelude → drop.
    assert!(mapper
        .tsx_range_to_carrier(TsPosition::new(2, 0), TsPosition::new(4, 5))
        .is_none());
}

#[test]
fn rewrite_region_drops_and_shifts_columns() {
    // `import x from './store.svelte';` — the specifier `'./store.svelte'`
    // is rewritten to `'./store.svelte.ts'` (the `.ts` provider suffix),
    // inserting 3 chars. The specifier byte span covers the quotes.
    let src = "import x from './store.svelte';\nexport const y = x;\n";
    // Byte span of `'./store.svelte'` (with quotes): find it.
    let needle = "'./store.svelte'";
    let start = src.find(needle).unwrap();
    let end = start + needle.len();
    let replacements = vec![(start, end, "'./store.svelte.ts'".to_string())];
    let mapper = SelfFileProviderMapper::new(4, &replacements, &li(src));

    let spec_col = src[..start].encode_utf16().count() as u32; // column where the specifier starts on line 0
    let spec_end_col = spec_col + needle.encode_utf16().count() as u32;

    // A column BEFORE the rewrite (`import x` region) is a pure +prelude
    // offset with NO column shift.
    let before = mapper
        .carrier_to_tsx(LspPosition::new(0, 0))
        .expect("col 0 maps");
    assert_eq!(before.pos, TsPosition::new(4, 0));

    // A column INSIDE the rewritten specifier is DROPPED (no wrong column).
    let inside = spec_col + 3;
    assert!(
        mapper.carrier_to_tsx(LspPosition::new(0, inside)).is_none(),
        "a position inside a rewritten specifier must drop, not return a wrong column"
    );

    // A column AFTER the specifier on the same line is shifted by the
    // rewrite delta (+3) on the provider side (source col `spec_end_col`
    // maps to provider col `spec_end_col + 3`), and round-trips.
    let after_src = spec_end_col; // the `;` right after the specifier
    let after_prov = mapper
        .carrier_to_tsx(LspPosition::new(0, after_src))
        .expect("a column after the rewrite maps");
    assert_eq!(
        after_prov.pos,
        TsPosition::new(4, spec_end_col + 3),
        "columns after a rewrite shift by the rewrite delta on the provider side"
    );
    // Round-trip provider → source undoes the shift.
    let back = mapper
        .tsx_to_carrier(after_prov.pos)
        .expect("the shifted provider col maps back");
    assert_eq!(back.pos, LspPosition::new(0, after_src));

    // A provider column INSIDE the (wider) rewritten specifier also drops.
    let prov_inside = spec_col + 5;
    assert!(
        mapper
            .tsx_to_carrier(TsPosition::new(4, prov_inside))
            .is_none(),
        "a provider position inside the rewritten specifier must drop"
    );
}

#[test]
fn second_line_rewrite_does_not_shift_other_lines() {
    // Two import lines, only the second rewritten. Line 0 columns are
    // untouched; line 1 columns after the rewrite shift.
    let src = "import a from './a';\nimport b from './b.svelte';\n";
    let needle = "'./b.svelte'";
    let start = src.find(needle).unwrap();
    let end = start + needle.len();
    let replacements = vec![(start, end, "'./b.svelte.ts'".to_string())];
    let mapper = SelfFileProviderMapper::new(4, &replacements, &li(src));

    // Line 0 col 10 unaffected (pure +prelude).
    let l0 = mapper.carrier_to_tsx(LspPosition::new(0, 10)).unwrap();
    assert_eq!(l0.pos, TsPosition::new(4, 10));

    // Line 1 col 0 unaffected by the later rewrite on the same line.
    let l1_start = mapper.carrier_to_tsx(LspPosition::new(1, 0)).unwrap();
    assert_eq!(l1_start.pos, TsPosition::new(5, 0));
}

#[test]
fn two_same_line_rewrites_shift_provider_bounds_by_cumulative_delta() {
    // TWO rewritten import specifiers on ONE physical source line (legal:
    // `import a from './a.svelte'; import b from './b.svelte';`). Both are
    // rewritten to the `.ts` provider suffix (+3 each). The provider-side
    // bounds of the SECOND rewrite must be shifted by the cumulative delta of
    // the FIRST rewrite on the same line; otherwise `provider_col_to_source`
    // mismaps positions inside/after the 2nd specifier.
    let src = "import a from './a.svelte'; import b from './b.svelte';\n";
    let needle_a = "'./a.svelte'";
    let needle_b = "'./b.svelte'";
    let start_a = src.find(needle_a).unwrap();
    let end_a = start_a + needle_a.len();
    let start_b = src.find(needle_b).unwrap();
    let end_b = start_b + needle_b.len();
    let replacements = vec![
        (start_a, end_a, "'./a.svelte.ts'".to_string()),
        (start_b, end_b, "'./b.svelte.ts'".to_string()),
    ];
    let mapper = SelfFileProviderMapper::new(4, &replacements, &li(src));

    // Source columns of the two specifiers and the regions around them.
    let a_col = src[..start_a].encode_utf16().count() as u32;
    let a_end_col = a_col + needle_a.encode_utf16().count() as u32; // src col of `;` after `'./a.svelte'`
    let b_col = src[..start_b].encode_utf16().count() as u32;
    let b_end_col = b_col + needle_b.encode_utf16().count() as u32; // src col of `;` after `'./b.svelte'`

    // Each `.ts` rewrite inserts 3 chars; the first shifts everything after it.
    const DELTA: u32 = 3;

    // The region BETWEEN the two rewrites (` import b from ` … specifically the
    // `;` right after the first specifier) maps with the FIRST rewrite's delta.
    let between_src = a_end_col;
    let between_prov = mapper
        .carrier_to_tsx(LspPosition::new(0, between_src))
        .expect("the region between the two rewrites maps");
    assert_eq!(
        between_prov.pos,
        TsPosition::new(4, between_src + DELTA),
        "the region between the rewrites shifts by the first rewrite's delta"
    );
    // Round-trip: provider → source undoes the first delta.
    let between_back = mapper
        .tsx_to_carrier(between_prov.pos)
        .expect("the between-region provider col maps back");
    assert_eq!(between_back.pos, LspPosition::new(0, between_src));

    // The region AFTER the SECOND rewrite (the trailing `;`) maps with the
    // CUMULATIVE delta of BOTH rewrites.
    let after_src = b_end_col;
    let after_prov = mapper
        .carrier_to_tsx(LspPosition::new(0, after_src))
        .expect("the region after the second rewrite maps");
    assert_eq!(
        after_prov.pos,
        TsPosition::new(4, after_src + 2 * DELTA),
        "the region after the 2nd rewrite shifts by the cumulative delta of both rewrites"
    );
    // Round-trip the after-region provider col back to source — this is the
    // exact column `provider_col_to_source` mismapped pre-fix (it compared the
    // provider col against the UNSHIFTED 2nd-segment bounds).
    let after_back = mapper
        .tsx_to_carrier(after_prov.pos)
        .expect("the after-region provider col maps back");
    assert_eq!(
        after_back.pos,
        LspPosition::new(0, after_src),
        "a provider position after the 2nd rewrite round-trips to the right source column"
    );

    // A provider position INSIDE the SECOND rewritten specifier must DROP. The
    // 2nd specifier's ACTUAL provider span is `[b_col + DELTA, b_col + DELTA +
    // provider_width)` (shifted by the first rewrite). The discriminating
    // column is one that lies inside the SHIFTED span but OUTSIDE the UNSHIFTED
    // `[b_col, b_col + provider_width)` bound the pre-fix mapper compared
    // against — `provider_col_to_source` accepts `col >= provider_end` (only
    // `< provider_end` drops), so pre-fix it returned a WRONG source column for
    // this position instead of dropping.
    let provider_width_b = "'./b.svelte.ts'".encode_utf16().count() as u32;
    let inside_2nd_shifted = b_col + provider_width_b + 1; // inside shifted span, past unshifted end
    assert!(
        inside_2nd_shifted < b_col + DELTA + provider_width_b,
        "the probe column must lie inside the SHIFTED 2nd specifier span"
    );
    assert!(
        inside_2nd_shifted >= b_col + provider_width_b,
        "the probe column must lie at/after the UNSHIFTED 2nd specifier end (the pre-fix gap)"
    );
    assert!(
        mapper
            .tsx_to_carrier(TsPosition::new(4, inside_2nd_shifted))
            .is_none(),
        "a provider position inside the 2nd rewritten specifier must drop, not return a wrong column"
    );
}

/// Guard `rune_module_self_file_projection_uses_prelude_line_count`: the
/// self-file projection's mapper applies the prelude line count as a uniform
/// DOWN-shift on source→provider and an UP-shift on provider→source. A
/// non-zero prelude is the discriminator — without consuming
/// `prelude_line_count` the provider line would equal the source line
/// (off-by-prelude).
#[test]
fn rune_module_self_file_projection_uses_prelude_line_count() {
    let src = "export const s = $state(0);\nexport const t = s;\n";
    for prelude in [1u32, 4, 9] {
        let mapper = SelfFileProviderMapper::new(prelude, &[], &li(src));
        assert_eq!(mapper.prelude_line_count(), prelude);
        // source line N → provider line N + prelude.
        let prov = mapper.carrier_to_tsx(LspPosition::new(1, 5)).unwrap();
        assert_eq!(
            prov.pos,
            TsPosition::new(1 + prelude, 5),
            "source→provider must shift the line down by prelude_line_count"
        );
        // provider line N+prelude → source line N (round-trip).
        let back = mapper.tsx_to_carrier(prov.pos).unwrap();
        assert_eq!(back.pos, LspPosition::new(1, 5));
        // The off-by-prelude bug would leave the line UNSHIFTED.
        assert_ne!(
            prov.pos.line, 1,
            "an unwired offset would map source line 1 to provider line 1 (off-by-prelude)"
        );
    }
}

/// Guard `self_file_mapper_drops_prelude_and_rewrite_regions`: the self-file
/// mapper DROPS provider positions in the prelude region (never clamps) and
/// DROPS positions inside a rewritten import-specifier span (never returns a
/// wrong column), and ranges require BOTH endpoints to map.
#[test]
fn self_file_mapper_drops_prelude_and_rewrite_regions() {
    let src = "import x from './store.svelte';\nexport const y = x;\n";
    let needle = "'./store.svelte'";
    let start = src.find(needle).unwrap();
    let end = start + needle.len();
    let replacements = vec![(start, end, "'./store.svelte.ts'".to_string())];
    let mapper = SelfFileProviderMapper::new(4, &replacements, &li(src));

    // Prelude region (provider lines < 4) drops, never clamps.
    for line in 0..4 {
        assert!(
            mapper.tsx_to_carrier(TsPosition::new(line, 0)).is_none(),
            "prelude line {line} must drop"
        );
    }

    // Inside the rewritten specifier (source side) drops.
    let spec_col = src[..start].encode_utf16().count() as u32;
    assert!(
        mapper
            .carrier_to_tsx(LspPosition::new(0, spec_col + 2))
            .is_none(),
        "a source position inside a rewritten specifier must drop"
    );
    // Inside the rewritten specifier (provider side) drops.
    assert!(
        mapper
            .tsx_to_carrier(TsPosition::new(4, spec_col + 2))
            .is_none(),
        "a provider position inside a rewritten specifier must drop"
    );

    // A range with one endpoint in the prelude region drops.
    assert!(
        mapper
            .tsx_range_to_carrier(TsPosition::new(2, 0), TsPosition::new(5, 1))
            .is_none(),
        "a range with an endpoint in the prelude region must drop"
    );
    // A range fully in the user region (line 1, no rewrite) maps.
    assert!(
        mapper
            .tsx_range_to_carrier(TsPosition::new(5, 0), TsPosition::new(5, 5))
            .is_some(),
        "a range fully in the user-source region maps"
    );
}
