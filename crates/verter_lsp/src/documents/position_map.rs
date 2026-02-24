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
        // Only return mappings that reference a source file
        token.get_source_id()?;
        Some(MappedPosition {
            line: token.get_src_line(),
            column: token.get_src_col(),
        })
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

        // Position between tokens (gen col 5) should snap to nearest token at or before
        let pos = mapper.tsx_to_vue(0, 5);
        assert!(pos.is_some());
        let pos = pos.unwrap();
        // lookup_token finds the closest token at or before the position
        assert_eq!(pos.line, 0);
        assert!(pos.column <= 5);
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
        // Column may snap to token start (0) since lookup_token returns the nearest token
        assert!(vue_pos.column <= 3);
    }
}
