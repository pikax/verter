//! Unit coverage for the single semantic-token legend-mapping owner.
//!
//! Discrimination notes: every expected value below is asymmetric against the
//! two historical failure modes — (a) decoding `"2020"` classifications as
//! `type | (mods << 8)` instead of `((type + 1) << 8) | mods`, and (b)
//! forwarding provider-space indices/bitsets to the wire without a name remap.

use super::*;

// ── decode_classification_2020 ───────────────────────────────────────────────

#[test]
fn decode_2020_splits_type_and_modifier_fields_with_plus_one_offset() {
    // interface(2) + declaration(bit 0): ((2 + 1) << 8) | 1 = 769.
    assert_eq!(decode_classification_2020(769), Some((2, 1)));
    // variable(7) + declaration|readonly|local (bits 0,3,5): 2048 | 41 = 2089.
    assert_eq!(decode_classification_2020(2089), Some((7, 0b10_1001)));
    // class(0), no modifiers: ((0 + 1) << 8) = 256 — the +1 offset is the only
    // thing distinguishing this from "no type field".
    assert_eq!(decode_classification_2020(256), Some((0, 0)));
}

#[test]
fn decode_2020_rejects_a_zero_type_field() {
    // Below 1 << 8 the +1-offset type field is zero: undecodable, never a
    // guessed (0, bits). An inverted decoder would happily return (bits, 0).
    assert_eq!(decode_classification_2020(0), None);
    assert_eq!(decode_classification_2020(0b10_1001), None);
    assert_eq!(decode_classification_2020(255), None);
}

// ── the fixed tsserver-2020 map ──────────────────────────────────────────────

#[test]
fn ts_2020_types_remap_by_name_not_by_index() {
    let map = SemanticTokenLegendMap::ts_classification_2020();
    // Same name, DIFFERENT index in each legend — an identity forward fails all
    // of these.
    for (ts_index, verter_index, name) in [
        (0, 2, "class"),
        (1, 3, "enum"),
        (2, 4, "interface"),
        (3, 0, "namespace"),
        (4, 6, "typeParameter"),
        (5, 1, "type"),
        (6, 7, "parameter"),
        (7, 8, "variable"),
        (8, 10, "enumMember"),
        (9, 9, "property"),
        (10, 12, "function"),
        (11, 13, "method"),
    ] {
        assert_eq!(
            map.map_token(ts_index, 0),
            Some((verter_index, 0)),
            "TS-2020 `{name}` ({ts_index}) must land on Verter `{name}` ({verter_index})"
        );
        assert_eq!(TS_CLASSIFICATION_2020_TOKEN_TYPES[ts_index as usize], name);
        assert_eq!(VERTER_TOKEN_TYPES[verter_index as usize], name);
    }
}

#[test]
fn ts_2020_modifier_bits_remap_individually_by_name() {
    let map = SemanticTokenLegendMap::ts_classification_2020();
    // TS bit → Verter bit, per name. The bitset is NOT forwarded as a unit.
    for (ts_bit, verter_bit, name) in [
        (0, 0, "declaration"),
        (1, 3, "static"),
        (2, 6, "async"),
        (3, 2, "readonly"),
        (4, 9, "defaultLibrary"),
        (5, 10, "local"),
    ] {
        assert_eq!(
            map.map_token(0, 1 << ts_bit),
            Some((2, 1 << verter_bit)),
            "TS-2020 modifier `{name}` (bit {ts_bit}) must land on Verter bit {verter_bit}"
        );
    }
    // A composite set recombines per-bit: declaration|readonly|local.
    assert_eq!(
        map.map_token(7, 0b10_1001),
        Some((8, (1 << 0) | (1 << 2) | (1 << 10))),
    );
}

#[test]
fn unknown_type_or_out_of_range_modifier_fails_closed() {
    let map = SemanticTokenLegendMap::ts_classification_2020();
    // Type index past the 12-entry TS legend.
    assert_eq!(map.map_token(12, 0), None);
    // Modifier bit past the 6-entry TS modifier legend — even with a valid type.
    assert_eq!(map.map_token(0, 1 << 6), None);
    // A valid modifier alongside an invalid one still drops the whole token.
    assert_eq!(map.map_token(0, (1 << 0) | (1 << 7)), None);
}

// ── name-built provider legends (the tsgo shape) ─────────────────────────────

/// The exact legend the pinned tsgo 7.0.2 advertised in a live `initialize`
/// probe (22 types — no `modifier` — in a server-owned order that matches
/// neither Verter's published order nor the client's advertised order).
const TSGO_7_0_2_TYPES: [&str; 22] = [
    "namespace",
    "class",
    "enum",
    "interface",
    "struct",
    "typeParameter",
    "type",
    "parameter",
    "variable",
    "property",
    "enumMember",
    "decorator",
    "event",
    "function",
    "method",
    "macro",
    "comment",
    "string",
    "keyword",
    "number",
    "regexp",
    "operator",
];

const TSGO_7_0_2_MODIFIERS: [&str; 11] = [
    "declaration",
    "definition",
    "readonly",
    "static",
    "deprecated",
    "abstract",
    "async",
    "modification",
    "documentation",
    "defaultLibrary",
    "local",
];

#[test]
fn tsgo_advertised_legend_remaps_into_verter_space() {
    let map = SemanticTokenLegendMap::from_names(&TSGO_7_0_2_TYPES, &TSGO_7_0_2_MODIFIERS);
    // tsgo `interface` = 3, Verter `interface` = 4 — the index shift that made
    // every carrier identifier the wrong color when forwarded raw.
    assert_eq!(map.map_token(3, 0), Some((4, 0)));
    // tsgo `function` = 13, Verter `function` = 12.
    assert_eq!(map.map_token(13, 0), Some((12, 0)));
    // tsgo `decorator` = 11, Verter `decorator` = 22.
    assert_eq!(map.map_token(11, 0), Some((22, 0)));
    // Modifiers: this tsgo order happens to match Verter's published order, so
    // the per-bit map is an identity HERE — by name, not by luck.
    assert_eq!(
        map.map_token(8, (1 << 0) | (1 << 2)),
        Some((8, (1 << 0) | (1 << 2))),
    );
    // Out-of-range for the 22-entry legend fails closed.
    assert_eq!(map.map_token(22, 0), None);
}

#[test]
fn a_name_verter_does_not_publish_fails_closed() {
    let map = SemanticTokenLegendMap::from_names(
        &["variable", "somethingNew"],
        &["declaration", "experimentalModifier"],
    );
    assert_eq!(map.map_token(0, 1), Some((8, 1)));
    // Unpublished TYPE name: drop.
    assert_eq!(map.map_token(1, 0), None);
    // Unpublished MODIFIER name: drop the token, not just the bit.
    assert_eq!(map.map_token(0, 1 << 1), None);
}

// ── JSON legend extraction ───────────────────────────────────────────────────

#[test]
fn legend_json_roundtrips_and_empty_legend_is_refused() {
    let legend = serde_json::json!({
        "tokenTypes": ["interface", "variable"],
        "tokenModifiers": ["declaration"],
    });
    let map = SemanticTokenLegendMap::from_legend_json(&legend).expect("legend-shaped");
    assert_eq!(map.map_token(0, 1), Some((4, 1)));
    assert_eq!(map.map_token(1, 0), Some((8, 0)));

    // The EMPTY legend a capability-less initialize produces must be refused —
    // retaining it would claim a negotiated legend that can map nothing.
    let empty = serde_json::json!({ "tokenTypes": [], "tokenModifiers": [] });
    assert!(SemanticTokenLegendMap::from_legend_json(&empty).is_none());
    // Non-legend shapes are refused, not defaulted.
    assert!(SemanticTokenLegendMap::from_legend_json(&serde_json::json!(null)).is_none());
    assert!(SemanticTokenLegendMap::from_legend_json(&serde_json::json!({})).is_none());
}

#[test]
fn initialize_result_extraction_reads_the_server_legend() {
    let init = serde_json::json!({
        "capabilities": {
            "semanticTokensProvider": {
                "legend": {
                    "tokenTypes": TSGO_7_0_2_TYPES,
                    "tokenModifiers": TSGO_7_0_2_MODIFIERS,
                },
                "range": true,
                "full": true,
            }
        },
        "serverInfo": { "name": "typescript-go", "version": "7.0.2" }
    });
    let map = SemanticTokenLegendMap::from_initialize_result(&init).expect("legend present");
    assert_eq!(map.map_token(3, 0), Some((4, 0)));

    // No semanticTokensProvider at all → None (fail closed at the caller).
    let bare = serde_json::json!({ "capabilities": {} });
    assert!(SemanticTokenLegendMap::from_initialize_result(&bare).is_none());
}

// ── the shared 2020 span walker ──────────────────────────────────────────────

#[test]
fn classified_spans_walk_decodes_maps_and_drops_unmappable() {
    let spans = vec![
        // "Shape": interface + declaration → Verter (4, 1).
        serde_json::json!(10),
        serde_json::json!(5),
        serde_json::json!(769),
        // Out-of-legend type index 12: ((12 + 1) << 8) → dropped.
        serde_json::json!(20),
        serde_json::json!(2),
        serde_json::json!(3328),
        // Zero type field (undecodable) → dropped.
        serde_json::json!(30),
        serde_json::json!(2),
        serde_json::json!(0),
        // "localCount": variable + declaration|readonly|local → Verter (8, 0b100_0000_0101).
        serde_json::json!(39),
        serde_json::json!(10),
        serde_json::json!(2089),
    ];
    let tokens = map_classified_spans_2020(&spans, None);
    assert_eq!(tokens.len(), 2, "{tokens:?}");
    assert_eq!(
        (
            tokens[0].start,
            tokens[0].length,
            tokens[0].token_type,
            tokens[0].token_modifiers
        ),
        (10, 5, 4, 1),
    );
    assert_eq!(
        (
            tokens[1].start,
            tokens[1].length,
            tokens[1].token_type,
            tokens[1].token_modifiers
        ),
        (39, 10, 8, (1 << 0) | (1 << 2) | (1 << 10)),
    );
}

#[test]
fn classified_spans_walk_ignores_trailing_partial_triplets() {
    let spans = vec![
        serde_json::json!(0),
        serde_json::json!(3),
        serde_json::json!(769),
        // Trailing [start, length] with no classification.
        serde_json::json!(9),
        serde_json::json!(1),
    ];
    let tokens = map_classified_spans_2020(&spans, None);
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].start, 0);
}

// @ai-generated - Guards checked conversion of absolute semantic-token starts.
#[test]
fn classified_spans_walk_drops_start_that_exceeds_u32() {
    let content = "const value = 1;";
    let wrapped_value_start = u64::from(u32::MAX) + 7;
    let spans = vec![
        // 4_294_967_302 truncates to 6 under `as u32`, which is the valid
        // byte/UTF-16 start of `value`. The malformed span must be dropped,
        // not emitted on that unrelated identifier.
        serde_json::json!(wrapped_value_start),
        serde_json::json!(5),
        serde_json::json!(((7 + 1) << 8) | 1),
    ];

    let tokens = map_classified_spans_2020(&spans, Some(content));
    assert!(
        tokens.is_empty(),
        "an overflowing start must yield no token, not wrap onto `value`: {tokens:?}"
    );
}

// @ai-generated - Guards fail-closed parsing of non-numeric semantic-token fields.
#[test]
fn classified_spans_walk_drops_non_numeric_field() {
    let content = "value";
    let spans = vec![
        // `unwrap_or(0)` fabricates a valid start at the first identifier.
        serde_json::json!("not-a-number"),
        serde_json::json!(5),
        serde_json::json!(((7 + 1) << 8) | 1),
    ];

    let tokens = map_classified_spans_2020(&spans, Some(content));
    assert!(
        tokens.is_empty(),
        "a non-numeric field must yield no token, not default onto byte 0: {tokens:?}"
    );
}

/// The engine's span offsets are UTF-16 CODE UNITS; the `SemanticToken`
/// contract is BYTES. With content available the walker converts — an
/// identifier after a multi-byte char lands at its true byte offset, and a
/// span past the end of the text drops instead of emitting a wrong position.
#[test]
fn classified_spans_walk_converts_utf16_offsets_to_bytes() {
    // "é" = 2 bytes / 1 UTF-16 unit; "𝛑" (U+1D6D1) = 4 bytes / 2 UTF-16 units.
    let content = "const é = 1;\nconst π𝛑val = 2;\n";
    // UTF-16 line 0 is 13 units; "π𝛑val" starts at UTF-16 unit 13 + 6 = 19,
    // spanning π(1) + 𝛑(2) + "val"(3) = 6 units. Its BYTE start is 14 + 6 = 20
    // (line 0 is 14 bytes incl. the 2-byte é + newline) and byte length is
    // 2 + 4 + 3 = 9.
    let spans = vec![
        // "é" @ utf16 6, len 1 — variable + declaration.
        serde_json::json!(6),
        serde_json::json!(1),
        serde_json::json!(((7 + 1) << 8) | 1),
        // "π𝛑val" @ utf16 19, len 6.
        serde_json::json!(19),
        serde_json::json!(6),
        serde_json::json!(((7 + 1) << 8) | 1),
        // A span past the end of the text: dropped, never mispositioned.
        serde_json::json!(1000),
        serde_json::json!(2),
        serde_json::json!(((7 + 1) << 8) | 1),
        // An overflowing UTF-16 end offset also fails closed.
        serde_json::json!(6),
        serde_json::json!(u32::MAX),
        serde_json::json!(((7 + 1) << 8) | 1),
    ];
    let tokens = map_classified_spans_2020(&spans, Some(content));
    assert_eq!(tokens.len(), 2, "{tokens:?}");
    assert_eq!((tokens[0].start, tokens[0].length), (6, 2), "é is 2 bytes");
    assert_eq!(
        &content[tokens[0].start as usize..(tokens[0].start + tokens[0].length) as usize],
        "é"
    );
    assert_eq!((tokens[1].start, tokens[1].length), (20, 9));
    assert_eq!(
        &content[tokens[1].start as usize..(tokens[1].start + tokens[1].length) as usize],
        "π𝛑val"
    );
}

// ── published-legend integrity ───────────────────────────────────────────────

#[test]
fn published_legend_has_no_duplicate_names() {
    let mut types: Vec<&str> = VERTER_TOKEN_TYPES.to_vec();
    types.sort_unstable();
    types.dedup();
    assert_eq!(types.len(), VERTER_TOKEN_TYPES.len());

    let mut modifiers: Vec<&str> = VERTER_TOKEN_MODIFIERS.to_vec();
    modifiers.sort_unstable();
    modifiers.dedup();
    assert_eq!(modifiers.len(), VERTER_TOKEN_MODIFIERS.len());
}

#[test]
fn every_ts_2020_name_is_publishable() {
    // The fixed tsserver map must be TOTAL: every classifier-2020 name exists
    // in Verter's published legend, so no tsserver token is ever dropped for a
    // vocabulary gap (drops are reserved for genuinely unknown vocabulary).
    let map = SemanticTokenLegendMap::ts_classification_2020();
    for index in 0..TS_CLASSIFICATION_2020_TOKEN_TYPES.len() as u32 {
        assert!(map.map_token(index, 0).is_some());
    }
    for bit in 0..TS_CLASSIFICATION_2020_TOKEN_MODIFIERS.len() as u32 {
        assert!(map.map_token(0, 1 << bit).is_some());
    }
}
