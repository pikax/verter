use oxc_sourcemap::SourceMap;

/// Bidirectional position mapper between Vue source positions and generated TSX positions.
///
/// Consumes an `oxc_sourcemap::SourceMap` (from `verter_host.get_tsx()`) and provides
/// lookups in both directions:
/// - `tsx_to_vue`: Generated TSX position -> Original Vue position (via `lookup_token`)
/// - `vue_to_tsx`: Original Vue position -> Generated TSX position (via sorted token scan)
///
/// All positions use 0-indexed lines and UTF-16 columns (matching LSP `Position`).
#[derive(Clone)]
pub struct PositionMapper {
    map: SourceMap,
}

/// A mapped position result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MappedPosition {
    /// 0-indexed line number.
    pub line: u32,
    /// 0-indexed UTF-16 column.
    pub column: u32,
}

impl PositionMapper {
    /// Create a position mapper from a source map JSON string.
    pub fn from_json(json: &str) -> Result<Self, String> {
        let map =
            SourceMap::from_json_string(json).map_err(|e| format!("invalid source map: {e}"))?;
        Ok(Self { map })
    }

    /// Map a generated TSX position back to the original Vue source position.
    ///
    /// This is the most common LSP operation: TSGO or TypeScript reports a position
    /// in the generated TSX, and we need the corresponding Vue source position.
    pub fn tsx_to_vue(&self, line: u32, column: u32) -> Option<MappedPosition> {
        let lookup_table = self.map.generate_lookup_table();
        let token = self.map.lookup_token(&lookup_table, line, column)?;

        // If the token has a source mapping, use it directly.
        if token.get_source_id().is_some() {
            let mut result = MappedPosition {
                line: token.get_src_line(),
                column: token.get_src_col(),
            };
            // Adjust column offset: if the query is past the token start on the same
            // generated line, add the difference. This provides character-level precision
            // within Original chunks (where source and generated content are identical).
            if token.get_dst_line() == line && column > token.get_dst_col() {
                result.column += column - token.get_dst_col();
            }
            return Some(result);
        }

        // Fallback: the token at this position has no source (e.g., synthetic `)` after
        // a binding name in a template expression). Scan backwards through tokens on the
        // same generated line to find the nearest preceding mapped token, then interpolate
        // the column offset — but ONLY if there's no intervening unmapped token between
        // them (which would indicate the query is inside synthetic text like `_ctx.`).
        let token_line = token.get_dst_line();
        let mut best: Option<MappedPosition> = None;
        let mut best_dst_col: u32 = 0;

        for t in self.map.get_tokens() {
            if t.get_dst_line() != token_line {
                continue;
            }
            if t.get_source_id().is_none() {
                continue;
            }
            let dst_col = t.get_dst_col();
            if dst_col > column {
                continue; // past the query position
            }
            if best.is_none() || dst_col > best_dst_col {
                best_dst_col = dst_col;
                best = Some(MappedPosition {
                    line: t.get_src_line(),
                    column: t.get_src_col(),
                });
            }
        }

        // Guard: if there's an unmapped token strictly between the best mapped token and
        // the query column, the query is inside synthetic text — return None.
        if best.is_some() {
            for t in self.map.get_tokens() {
                if t.get_dst_line() != token_line {
                    continue;
                }
                if t.get_source_id().is_some() {
                    continue;
                }
                let dst_col = t.get_dst_col();
                if dst_col > best_dst_col && dst_col < column {
                    return None;
                }
            }
        }

        // Interpolate: add the column distance from the best token to the query
        if let Some(ref mut pos) = best {
            if column > best_dst_col {
                pos.column += column - best_dst_col;
            }
        }

        best
    }

    /// Map an original Vue source position to the generated TSX position.
    ///
    /// This is needed when the user interacts at a Vue position and we need
    /// to query TSGO at the corresponding TSX offset.
    ///
    /// Scans all tokens to find the best match for the source position.
    /// Returns the generated position of the closest token at or before the given source position.
    pub fn vue_to_tsx(&self, line: u32, column: u32) -> Option<MappedPosition> {
        let mut best: Option<MappedPosition> = None;
        let mut best_src_line = u32::MAX;
        let mut best_src_col = u32::MAX;

        for token in self.map.get_tokens() {
            // Only consider tokens that map to a source
            if token.get_source_id().is_none() {
                continue;
            }

            let src_line = token.get_src_line();
            let src_col = token.get_src_col();

            // Skip tokens after the target position
            if src_line > line || (src_line == line && src_col > column) {
                continue;
            }

            // Find the token closest to the target (highest src position <= target)
            if best.is_none()
                || src_line > best_src_line
                || (src_line == best_src_line && src_col > best_src_col)
            {
                best_src_line = src_line;
                best_src_col = src_col;
                best = Some(MappedPosition {
                    line: token.get_dst_line(),
                    column: token.get_dst_col(),
                });
            }
        }

        // Adjust column offset: if the target is past the token start,
        // add the difference to the generated column
        if let Some(ref mut pos) = best {
            if best_src_line == line && column > best_src_col {
                pos.column += column - best_src_col;
            }
        }

        best
    }

    /// Get the underlying source map (for advanced queries).
    pub fn source_map(&self) -> &SourceMap {
        &self.map
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    // ========================================================================
    // TDD: Basic construction tests
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
        assert!(mapper.tsx_to_vue(0, 0).is_none());
    }

    // ========================================================================
    // TDD: tsx_to_vue (generated -> source) position mapping
    // ========================================================================

    #[test]
    fn test_tsx_to_vue_exact_token_match() {
        // Source map: gen(0,0) -> src(2,5), gen(1,0) -> src(4,0)
        let json = build_test_source_map(
            "App.vue",
            "<template>\n  <div>hello</div>\n</template>\n\n<script setup>\nconst x = 1;\n</script>",
            &[(0, 0, 2, 5), (1, 0, 4, 0)],
        );
        let mapper = PositionMapper::from_json(&json).unwrap();

        // Exact match on first token
        let pos = mapper.tsx_to_vue(0, 0).unwrap();
        assert_eq!(pos, MappedPosition { line: 2, column: 5 });

        // Exact match on second token
        let pos = mapper.tsx_to_vue(1, 0).unwrap();
        assert_eq!(pos, MappedPosition { line: 4, column: 0 });
    }

    #[test]
    fn test_tsx_to_vue_between_tokens() {
        // Source map: gen(0,0) -> src(0,0), gen(0,10) -> src(0,10)
        let json = build_test_source_map(
            "App.vue",
            "const x = hello;",
            &[(0, 0, 0, 0), (0, 10, 0, 10)],
        );
        let mapper = PositionMapper::from_json(&json).unwrap();

        // Position between tokens (gen col 5) — finds token at gen(0,0)->src(0,0),
        // then adjusts column by the offset: 0 + (5 - 0) = 5
        let pos = mapper.tsx_to_vue(0, 5);
        assert!(pos.is_some());
        let pos = pos.unwrap();
        assert_eq!(pos.line, 0);
        assert_eq!(pos.column, 5);
    }

    // ========================================================================
    // TDD: vue_to_tsx (source -> generated) position mapping
    // ========================================================================

    #[test]
    fn test_vue_to_tsx_exact_token_match() {
        // Source map: gen(5,0) -> src(0,0), gen(6,4) -> src(1,2)
        let json =
            build_test_source_map("App.vue", "hello\n  world", &[(5, 0, 0, 0), (6, 4, 1, 2)]);
        let mapper = PositionMapper::from_json(&json).unwrap();

        // src(0,0) -> gen(5,0)
        let pos = mapper.vue_to_tsx(0, 0).unwrap();
        assert_eq!(pos, MappedPosition { line: 5, column: 0 });

        // src(1,2) -> gen(6,4)
        let pos = mapper.vue_to_tsx(1, 2).unwrap();
        assert_eq!(pos, MappedPosition { line: 6, column: 4 });
    }

    #[test]
    fn test_vue_to_tsx_offset_within_token_range() {
        // Source map: gen(0,0) -> src(0,0), gen(0,10) -> src(0,10)
        // Query src(0,3) — between two tokens on same line
        let json =
            build_test_source_map("App.vue", "const x = 1;", &[(0, 0, 0, 0), (0, 10, 0, 10)]);
        let mapper = PositionMapper::from_json(&json).unwrap();

        // src(0,3) should map to gen(0,3) (offset from token at src(0,0)->gen(0,0))
        let pos = mapper.vue_to_tsx(0, 3).unwrap();
        assert_eq!(pos, MappedPosition { line: 0, column: 3 });
    }

    #[test]
    fn test_vue_to_tsx_no_tokens() {
        let json = build_test_source_map("App.vue", "", &[]);
        let mapper = PositionMapper::from_json(&json).unwrap();

        let pos = mapper.vue_to_tsx(0, 0);
        assert!(pos.is_none());
    }

    #[test]
    fn test_vue_to_tsx_position_before_all_tokens() {
        // All tokens start at src(2,0) or later
        let json = build_test_source_map("App.vue", "\n\nhello", &[(5, 0, 2, 0)]);
        let mapper = PositionMapper::from_json(&json).unwrap();

        // src(0,0) is before all tokens
        let pos = mapper.vue_to_tsx(0, 0);
        assert!(pos.is_none());
    }

    #[test]
    fn test_vue_to_tsx_multiline_source() {
        // Simulates a script block starting at line 5 in the Vue file,
        // mapped to line 0 in TSX
        let json = build_test_source_map(
            "App.vue",
            "<template>\n  <div/>\n</template>\n\n<script setup>\nconst x = 1;\nconst y = 2;\n</script>",
            &[
                (0, 0, 5, 0),   // gen(0,0) -> src(5,0)  "const x = 1;"
                (1, 0, 6, 0),   // gen(1,0) -> src(6,0)  "const y = 2;"
            ],
        );
        let mapper = PositionMapper::from_json(&json).unwrap();

        // src(5,6) should map to gen(0,6) — "x" in "const x = 1;"
        let pos = mapper.vue_to_tsx(5, 6).unwrap();
        assert_eq!(pos, MappedPosition { line: 0, column: 6 });

        // src(6,6) should map to gen(1,6)
        let pos = mapper.vue_to_tsx(6, 6).unwrap();
        assert_eq!(pos, MappedPosition { line: 1, column: 6 });
    }

    // ========================================================================
    // TDD: Roundtrip tests (vue -> tsx -> vue)
    // ========================================================================

    #[test]
    fn test_roundtrip_exact_token_positions() {
        // 1:1 identity mapping (common for script blocks)
        let json = build_test_source_map(
            "App.vue",
            "const x = 1;\nconst y = 2;",
            &[(0, 0, 0, 0), (1, 0, 1, 0)],
        );
        let mapper = PositionMapper::from_json(&json).unwrap();

        // Roundtrip: vue(0,0) -> tsx -> vue
        let tsx_pos = mapper.vue_to_tsx(0, 0).unwrap();
        let vue_pos = mapper.tsx_to_vue(tsx_pos.line, tsx_pos.column).unwrap();
        assert_eq!(vue_pos, MappedPosition { line: 0, column: 0 });

        // Roundtrip: vue(1,0) -> tsx -> vue
        let tsx_pos = mapper.vue_to_tsx(1, 0).unwrap();
        let vue_pos = mapper.tsx_to_vue(tsx_pos.line, tsx_pos.column).unwrap();
        assert_eq!(vue_pos, MappedPosition { line: 1, column: 0 });
    }

    #[test]
    fn test_roundtrip_with_offset() {
        // Script at Vue line 10 -> TSX line 0
        let json = build_test_source_map(
            "App.vue",
            &" ".repeat(200), // dummy content
            &[
                (0, 0, 10, 0), // gen(0,0) -> src(10,0)
                (0, 6, 10, 6), // gen(0,6) -> src(10,6)
                (1, 0, 11, 0), // gen(1,0) -> src(11,0)
            ],
        );
        let mapper = PositionMapper::from_json(&json).unwrap();

        // vue(10,3) -> tsx(0,3) -> vue(10,0) (snaps to token start)
        let tsx_pos = mapper.vue_to_tsx(10, 3).unwrap();
        assert_eq!(tsx_pos.line, 0);
        assert_eq!(tsx_pos.column, 3);

        let vue_pos = mapper.tsx_to_vue(tsx_pos.line, tsx_pos.column).unwrap();
        assert_eq!(vue_pos.line, 10);
        // Column adjustment: token at gen(0,0)->src(10,0), query gen(0,3) -> src(10, 0 + 3) = 3
        assert_eq!(vue_pos.column, 3);
    }

    // ========================================================================
    // TDD: Prepended text source map accuracy (e.g., _ctx. / $setup. prefixes)
    // ========================================================================

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

    /// Simulates: Vue source "  count" at line 1, col 3
    /// TSX output: "  _ctx.count" — "_ctx." prepended at the position of "count"
    /// Source map tokens:
    ///   gen(0,0) -> src(1,0)  — mapped (the "  " spaces, Original chunk)
    ///   gen(0,2) -> unmapped  — the "_ctx." prefix (Inserted chunk)
    ///   gen(0,7) -> src(1,2)  — mapped (the "count" content, Original chunk)
    #[test]
    fn test_prepended_text_forward_mapping_start_middle_end() {
        let json = build_source_map_with_unmapped(
            "App.vue",
            // Full source content (Vue file)
            "<template>\n   count\n</template>",
            &[
                (0, 0, 1, 0), // gen(0,0) -> src(1,0): spaces before count
                (0, 7, 1, 3), // gen(0,7) -> src(1,3): "count" starts (after "_ctx.")
            ],
            &[
                (0, 2), // unmapped: "_ctx." inserted at gen col 2
            ],
        );
        let mapper = PositionMapper::from_json(&json).unwrap();

        // Forward: start of "count" in Vue (line 1, col 3) -> TSX (line 0, col 7)
        let tsx_pos = mapper.vue_to_tsx(1, 3).unwrap();
        assert_eq!(tsx_pos, MappedPosition { line: 0, column: 7 });

        // Forward: middle of "count" (the 'u', col 5) -> TSX col 9
        let tsx_pos = mapper.vue_to_tsx(1, 5).unwrap();
        assert_eq!(tsx_pos, MappedPosition { line: 0, column: 9 });

        // Forward: end of "count" (the 't', col 7) -> TSX col 11
        let tsx_pos = mapper.vue_to_tsx(1, 7).unwrap();
        assert_eq!(
            tsx_pos,
            MappedPosition {
                line: 0,
                column: 11
            }
        );
    }

    #[test]
    fn test_prepended_text_reverse_mapping_start_middle_end() {
        let json = build_source_map_with_unmapped(
            "App.vue",
            "<template>\n   count\n</template>",
            &[
                (0, 0, 1, 0), // spaces
                (0, 7, 1, 3), // "count"
            ],
            &[
                (0, 2), // "_ctx." unmapped
            ],
        );
        let mapper = PositionMapper::from_json(&json).unwrap();

        // Reverse: TSX col 7 ('c' of count) -> Vue (1, 3)
        let vue_pos = mapper.tsx_to_vue(0, 7).unwrap();
        assert_eq!(vue_pos, MappedPosition { line: 1, column: 3 });

        // Reverse: TSX col 9 ('u' of count) -> Vue (1, 5) — with column adjustment
        let vue_pos = mapper.tsx_to_vue(0, 9).unwrap();
        assert_eq!(vue_pos, MappedPosition { line: 1, column: 5 });

        // Reverse: TSX col 11 ('t' of count) -> Vue (1, 7)
        let vue_pos = mapper.tsx_to_vue(0, 11).unwrap();
        assert_eq!(vue_pos, MappedPosition { line: 1, column: 7 });
    }

    #[test]
    fn test_prepended_text_inside_prefix_returns_none() {
        let json = build_source_map_with_unmapped(
            "App.vue",
            "<template>\n   count\n</template>",
            &[(0, 0, 1, 0), (0, 7, 1, 3)],
            &[
                (0, 2), // "_ctx." unmapped at gen col 2
            ],
        );
        let mapper = PositionMapper::from_json(&json).unwrap();

        // Inside "_ctx." prefix (gen cols 2-6): should return None (unmapped)
        let pos = mapper.tsx_to_vue(0, 3);
        assert!(
            pos.is_none(),
            "Hovering inside prepended '_ctx.' should return None, got: {:?}",
            pos
        );

        let pos = mapper.tsx_to_vue(0, 5);
        assert!(
            pos.is_none(),
            "Hovering at '_ctx.' dot should return None, got: {:?}",
            pos
        );
    }

    #[test]
    fn test_prepended_text_roundtrip_character_level() {
        // Simulate $setup.count: gen "$setup.count" with "$setup." prepended
        let json = build_source_map_with_unmapped(
            "App.vue",
            "<script setup>\nconst count = 0\n</script>\n<template>{{ count }}</template>",
            &[
                (0, 7, 3, 14), // gen(0,7) -> src(3,14): "count" in template
            ],
            &[
                (0, 0), // "$setup." unmapped
            ],
        );
        let mapper = PositionMapper::from_json(&json).unwrap();

        // Roundtrip every character of "count" (5 chars)
        for i in 0..5u32 {
            let vue_col = 14 + i;
            let tsx_pos = mapper.vue_to_tsx(3, vue_col);
            assert!(
                tsx_pos.is_some(),
                "vue_to_tsx should succeed for 'count'[{i}] at col {vue_col}"
            );
            let tsx_pos = tsx_pos.unwrap();
            assert_eq!(tsx_pos.line, 0);
            assert_eq!(
                tsx_pos.column,
                7 + i,
                "Character {i} of 'count' should map to TSX col {}",
                7 + i
            );

            // Reverse: TSX -> Vue
            let vue_pos = mapper.tsx_to_vue(tsx_pos.line, tsx_pos.column);
            assert!(
                vue_pos.is_some(),
                "tsx_to_vue should succeed for TSX col {}",
                tsx_pos.column
            );
            let vue_pos = vue_pos.unwrap();
            assert_eq!(vue_pos.line, 3);
            assert_eq!(
                vue_pos.column, vue_col,
                "Roundtrip for 'count'[{i}]: expected Vue col {vue_col}, got {}",
                vue_pos.column
            );
        }
    }

    #[test]
    fn test_prepended_text_multiple_bindings_on_same_line() {
        // Simulates: "{{ a }} {{ b }}" -> "$setup.a $setup.b"
        // Vue line 2: "{{ a }} {{ b }}"  (a at col 3, b at col 11)
        // TSX line 0: "$setup.a $setup.b"
        //              0123456789...
        let json = build_source_map_with_unmapped(
            "App.vue",
            "<template>\n<div>{{ a }} {{ b }}</div>\n</template>",
            &[
                (0, 7, 1, 9),   // gen(0,7) -> src(1,9): "a"
                (0, 9, 1, 11),  // gen(0,9) -> src(1,11): " " between interpolations
                (0, 16, 1, 18), // gen(0,16) -> src(1,18): "b"
            ],
            &[
                (0, 0), // "$setup." for a
                (0, 8), // unmapped space/separator (end of a region)
                (0, 9), // "$setup." for b
            ],
        );
        let mapper = PositionMapper::from_json(&json).unwrap();

        // Forward: "a" at Vue (1,9) -> TSX (0,7)
        let pos = mapper.vue_to_tsx(1, 9).unwrap();
        assert_eq!(pos, MappedPosition { line: 0, column: 7 });

        // Forward: "b" at Vue (1,18) -> TSX (0,16)
        let pos = mapper.vue_to_tsx(1, 18).unwrap();
        assert_eq!(
            pos,
            MappedPosition {
                line: 0,
                column: 16
            }
        );

        // Reverse: TSX col 7 -> Vue (1,9) for "a"
        let pos = mapper.tsx_to_vue(0, 7).unwrap();
        assert_eq!(pos, MappedPosition { line: 1, column: 9 });

        // Reverse: TSX col 16 -> Vue (1,18) for "b"
        let pos = mapper.tsx_to_vue(0, 16).unwrap();
        assert_eq!(
            pos,
            MappedPosition {
                line: 1,
                column: 18
            }
        );
    }

    // ========================================================================
    // TDD: UTF-16 position mapping (multi-byte characters)
    // ========================================================================

    /// Simulates a source where emoji (4 bytes UTF-8, 2 UTF-16 code units) appears
    /// before a binding. Source map columns are in UTF-16.
    ///
    /// Vue source line: "😀{{ msg }}" — emoji at col 0-1 (UTF-16), "msg" at col 5
    /// TSX output: "$setup.msg" — "msg" at gen col 7
    #[test]
    fn test_utf16_emoji_before_binding_forward() {
        // Source: "😀{{ msg }}" — emoji is 2 UTF-16 units
        // In UTF-16 columns: 😀=0..1, {{=2..3, space=4, msg=5..7
        // TSX: "$setup.msg" — $setup.=0..6, msg=7..9
        let json = build_source_map_with_unmapped(
            "App.vue",
            "😀{{ msg }}",
            &[
                (0, 7, 0, 5), // gen(0,7) -> src(0,5): "msg" start (UTF-16 col 5)
            ],
            &[
                (0, 0), // "$setup." prefix unmapped
            ],
        );
        let mapper = PositionMapper::from_json(&json).unwrap();

        // Forward: "msg"[0] at Vue col 5 -> TSX col 7
        let pos = mapper.vue_to_tsx(0, 5).unwrap();
        assert_eq!(pos, MappedPosition { line: 0, column: 7 });

        // Forward: "msg"[1] at Vue col 6 -> TSX col 8
        let pos = mapper.vue_to_tsx(0, 6).unwrap();
        assert_eq!(pos, MappedPosition { line: 0, column: 8 });

        // Forward: "msg"[2] at Vue col 7 -> TSX col 9
        let pos = mapper.vue_to_tsx(0, 7).unwrap();
        assert_eq!(pos, MappedPosition { line: 0, column: 9 });
    }

    #[test]
    fn test_utf16_emoji_before_binding_reverse() {
        let json = build_source_map_with_unmapped(
            "App.vue",
            "😀{{ msg }}",
            &[
                (0, 7, 0, 5), // gen(0,7) -> src(0,5): "msg"
            ],
            &[
                (0, 0), // "$setup." prefix
            ],
        );
        let mapper = PositionMapper::from_json(&json).unwrap();

        // Reverse: TSX col 7 -> Vue col 5 (exact match)
        let pos = mapper.tsx_to_vue(0, 7).unwrap();
        assert_eq!(pos, MappedPosition { line: 0, column: 5 });

        // Reverse: TSX col 8 -> Vue col 6 (column adjustment)
        let pos = mapper.tsx_to_vue(0, 8).unwrap();
        assert_eq!(pos, MappedPosition { line: 0, column: 6 });

        // Reverse: TSX col 9 -> Vue col 7
        let pos = mapper.tsx_to_vue(0, 9).unwrap();
        assert_eq!(pos, MappedPosition { line: 0, column: 7 });
    }

    /// Simulates a binding name containing a non-ASCII character (café).
    /// 'é' is 2 bytes in UTF-8 but 1 UTF-16 code unit.
    ///
    /// Vue source: "{{ café }}" — c=col3, a=col4, f=col5, é=col6
    /// TSX: "_ctx.café" — _ctx.=0..4, c=col5, a=col6, f=col7, é=col8
    #[test]
    fn test_utf16_non_ascii_identifier_roundtrip() {
        // café: 4 UTF-16 units (c=1, a=1, f=1, é=1)
        let json = build_source_map_with_unmapped(
            "App.vue",
            "{{ café }}",
            &[
                (0, 5, 0, 3), // gen(0,5) -> src(0,3): "café" starts
            ],
            &[
                (0, 0), // "_ctx." prefix
            ],
        );
        let mapper = PositionMapper::from_json(&json).unwrap();

        // Roundtrip each character: c(0), a(1), f(2), é(3)
        for i in 0..4u32 {
            let vue_col = 3 + i;
            let tsx_pos = mapper.vue_to_tsx(0, vue_col).unwrap();
            assert_eq!(
                tsx_pos.column,
                5 + i,
                "café[{i}] forward: expected TSX col {}",
                5 + i
            );

            let vue_roundtrip = mapper.tsx_to_vue(0, tsx_pos.column).unwrap();
            assert_eq!(
                vue_roundtrip.column, vue_col,
                "café[{i}] roundtrip: expected Vue col {vue_col}"
            );
        }
    }

    /// Surrogate pair (emoji 😀 = 4 bytes UTF-8, 2 UTF-16 units) inside an identifier.
    /// Variable name "a😀b": a=1 unit, 😀=2 units, b=1 unit = 4 UTF-16 units.
    #[test]
    fn test_utf16_surrogate_pair_in_identifier() {
        // Source: "a😀b" (4 UTF-16 units), TSX: "_ctx.a😀b"
        // Source cols: a=3, 😀=4-5, b=6
        // TSX cols: _ctx.=0..4, a=5, 😀=6-7, b=8
        let json = build_source_map_with_unmapped(
            "App.vue",
            "{{ a😀b }}",
            &[
                (0, 5, 0, 3), // gen(0,5) -> src(0,3): "a😀b" starts
            ],
            &[
                (0, 0), // "_ctx." prefix
            ],
        );
        let mapper = PositionMapper::from_json(&json).unwrap();

        // 'a' at Vue col 3 -> TSX col 5
        let pos = mapper.vue_to_tsx(0, 3).unwrap();
        assert_eq!(pos.column, 5);
        let back = mapper.tsx_to_vue(0, 5).unwrap();
        assert_eq!(back.column, 3);

        // 😀 first unit at Vue col 4 -> TSX col 6
        let pos = mapper.vue_to_tsx(0, 4).unwrap();
        assert_eq!(pos.column, 6);
        let back = mapper.tsx_to_vue(0, 6).unwrap();
        assert_eq!(back.column, 4);

        // 😀 second unit at Vue col 5 -> TSX col 7
        let pos = mapper.vue_to_tsx(0, 5).unwrap();
        assert_eq!(pos.column, 7);
        let back = mapper.tsx_to_vue(0, 7).unwrap();
        assert_eq!(back.column, 5);

        // 'b' at Vue col 6 -> TSX col 8
        let pos = mapper.vue_to_tsx(0, 6).unwrap();
        assert_eq!(pos.column, 8);
        let back = mapper.tsx_to_vue(0, 8).unwrap();
        assert_eq!(back.column, 6);
    }
}
