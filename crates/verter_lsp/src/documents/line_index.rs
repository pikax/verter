use tower_lsp_server::lsp_types::Position;

/// Precomputed line start offsets for fast byte-offset ↔ LSP Position conversion.
///
/// Handles UTF-16 code units as required by the LSP protocol specification.
/// Build once per document version, then use for all position conversions.
#[derive(Debug, Clone)]
pub struct LineIndex {
    /// Byte offset of the start of each line. `line_starts[0]` is always 0.
    line_starts: Vec<u32>,
    /// The full source text (needed for UTF-16 column calculation).
    source: Vec<u8>,
}

impl LineIndex {
    /// Build a `LineIndex` from the full source text.
    pub fn new(source: &str) -> Self {
        let bytes = source.as_bytes();
        let mut line_starts = vec![0u32];
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'\n' {
                line_starts.push((i + 1) as u32);
            }
        }
        Self {
            line_starts,
            source: bytes.to_vec(),
        }
    }

    /// Convert a byte offset to an LSP `Position` (0-indexed line, UTF-16 column).
    ///
    /// Returns `None` if the offset is out of bounds.
    pub fn offset_to_position(&self, offset: u32) -> Option<Position> {
        let offset = offset as usize;
        if offset > self.source.len() {
            return None;
        }

        // Binary search for the line containing this offset
        let line = match self.line_starts.binary_search(&(offset as u32)) {
            Ok(exact) => exact,
            Err(insert) => insert - 1,
        };

        let line_start = self.line_starts[line] as usize;
        let col_bytes = &self.source[line_start..offset];
        let col_utf16 = utf8_byte_len_to_utf16_len(col_bytes);

        Some(Position {
            line: line as u32,
            character: col_utf16 as u32,
        })
    }

    /// Convert an LSP `Position` (0-indexed line, UTF-16 column) to a byte offset.
    ///
    /// Returns `None` if the position is out of bounds.
    pub fn position_to_offset(&self, pos: &Position) -> Option<u32> {
        let line = pos.line as usize;
        if line >= self.line_starts.len() {
            return None;
        }

        let line_start = self.line_starts[line] as usize;
        let line_end = if line + 1 < self.line_starts.len() {
            self.line_starts[line + 1] as usize
        } else {
            self.source.len()
        };

        let line_bytes = &self.source[line_start..line_end];
        let byte_col = utf16_col_to_byte_col(line_bytes, pos.character as usize);

        let offset = line_start + byte_col;
        if offset > self.source.len() {
            return None;
        }
        Some(offset as u32)
    }

    /// Return the number of lines in the source.
    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    /// Return the byte offset of the start of a line.
    pub fn line_start(&self, line: usize) -> Option<u32> {
        self.line_starts.get(line).copied()
    }

    /// Return the byte offset of the end of a line (before the newline, or EOF).
    pub fn line_end(&self, line: usize) -> Option<u32> {
        let start = self.line_start(line)? as usize;
        let end = if line + 1 < self.line_starts.len() {
            // Exclude the trailing newline
            let next_start = self.line_starts[line + 1] as usize;
            if next_start > 0 && self.source.get(next_start - 1) == Some(&b'\n') {
                if next_start > 1 && self.source.get(next_start - 2) == Some(&b'\r') {
                    next_start - 2
                } else {
                    next_start - 1
                }
            } else {
                next_start
            }
        } else {
            self.source.len()
        };
        let _ = start; // suppress warning; we only need end
        Some(end as u32)
    }
}

/// Count the number of UTF-16 code units needed to represent a byte slice.
fn utf8_byte_len_to_utf16_len(bytes: &[u8]) -> usize {
    let s = match std::str::from_utf8(bytes) {
        Ok(s) => s,
        Err(_) => return bytes.len(), // fallback: assume ASCII
    };
    s.encode_utf16().count()
}

/// Convert a UTF-16 column offset to a byte column offset within a line.
fn utf16_col_to_byte_col(line_bytes: &[u8], utf16_col: usize) -> usize {
    let line_str = match std::str::from_utf8(line_bytes) {
        Ok(s) => s,
        Err(_) => return utf16_col.min(line_bytes.len()), // fallback
    };

    let mut utf16_count = 0;
    for (byte_idx, ch) in line_str.char_indices() {
        if utf16_count >= utf16_col {
            return byte_idx;
        }
        utf16_count += ch.len_utf16();
    }

    // If we ran past the end, clamp to line length
    line_str.len().min(line_bytes.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // Basic construction
    // ========================================================================

    #[test]
    fn test_empty_source() {
        let idx = LineIndex::new("");
        assert_eq!(idx.line_count(), 1);
        assert_eq!(
            idx.offset_to_position(0),
            Some(Position {
                line: 0,
                character: 0
            })
        );
    }

    #[test]
    fn test_single_line() {
        let idx = LineIndex::new("hello");
        assert_eq!(idx.line_count(), 1);
        assert_eq!(
            idx.offset_to_position(0),
            Some(Position {
                line: 0,
                character: 0
            })
        );
        assert_eq!(
            idx.offset_to_position(5),
            Some(Position {
                line: 0,
                character: 5
            })
        );
    }

    #[test]
    fn test_multiple_lines() {
        let idx = LineIndex::new("abc\ndef\nghi");
        assert_eq!(idx.line_count(), 3);

        // Start of each line
        assert_eq!(
            idx.offset_to_position(0),
            Some(Position {
                line: 0,
                character: 0
            })
        );
        assert_eq!(
            idx.offset_to_position(4),
            Some(Position {
                line: 1,
                character: 0
            })
        );
        assert_eq!(
            idx.offset_to_position(8),
            Some(Position {
                line: 2,
                character: 0
            })
        );

        // Middle of a line
        assert_eq!(
            idx.offset_to_position(5),
            Some(Position {
                line: 1,
                character: 1
            })
        );
    }

    #[test]
    fn test_trailing_newline() {
        let idx = LineIndex::new("abc\n");
        assert_eq!(idx.line_count(), 2);
        // Position after the newline is start of line 1
        assert_eq!(
            idx.offset_to_position(4),
            Some(Position {
                line: 1,
                character: 0
            })
        );
    }

    // ========================================================================
    // offset_to_position edge cases
    // ========================================================================

    #[test]
    fn test_offset_at_newline() {
        let idx = LineIndex::new("abc\ndef");
        // Offset 3 is the '\n' character — still on line 0
        assert_eq!(
            idx.offset_to_position(3),
            Some(Position {
                line: 0,
                character: 3
            })
        );
    }

    #[test]
    fn test_offset_out_of_bounds() {
        let idx = LineIndex::new("abc");
        assert!(idx.offset_to_position(4).is_none());
    }

    #[test]
    fn test_offset_at_eof() {
        let idx = LineIndex::new("abc");
        // Offset == len is valid (one past last char)
        assert_eq!(
            idx.offset_to_position(3),
            Some(Position {
                line: 0,
                character: 3
            })
        );
    }

    // ========================================================================
    // position_to_offset
    // ========================================================================

    #[test]
    fn test_position_to_offset_basic() {
        let idx = LineIndex::new("abc\ndef\nghi");
        assert_eq!(
            idx.position_to_offset(&Position {
                line: 0,
                character: 0
            }),
            Some(0)
        );
        assert_eq!(
            idx.position_to_offset(&Position {
                line: 1,
                character: 0
            }),
            Some(4)
        );
        assert_eq!(
            idx.position_to_offset(&Position {
                line: 2,
                character: 2
            }),
            Some(10)
        );
    }

    #[test]
    fn test_position_to_offset_invalid_line() {
        let idx = LineIndex::new("abc");
        assert!(idx
            .position_to_offset(&Position {
                line: 5,
                character: 0
            })
            .is_none());
    }

    // ========================================================================
    // Roundtrip: offset -> position -> offset
    // ========================================================================

    #[test]
    fn test_roundtrip_ascii() {
        let source = "<script setup>\nconst x = ref(0);\nconst y = 'hello';\n</script>";
        let idx = LineIndex::new(source);

        for offset in 0..source.len() as u32 {
            let pos = idx.offset_to_position(offset).unwrap();
            let back = idx.position_to_offset(&pos).unwrap();
            assert_eq!(back, offset, "roundtrip failed for offset {offset}");
        }
    }

    // ========================================================================
    // UTF-16 handling
    // ========================================================================

    #[test]
    fn test_utf16_bmp_character() {
        // é is 2 bytes in UTF-8 but 1 UTF-16 code unit
        let idx = LineIndex::new("café");
        // 'c' = offset 0, 'a' = 1, 'f' = 2, 'é' = 3-4
        assert_eq!(
            idx.offset_to_position(3),
            Some(Position {
                line: 0,
                character: 3
            })
        );
        // After 'é' (offset 5, which is byte len)
        assert_eq!(
            idx.offset_to_position(5),
            Some(Position {
                line: 0,
                character: 4
            })
        );
    }

    #[test]
    fn test_utf16_supplementary_character() {
        // 😀 is 4 bytes in UTF-8 and 2 UTF-16 code units (surrogate pair)
        let idx = LineIndex::new("a😀b");
        // 'a' = offset 0, '😀' = offset 1-4, 'b' = offset 5
        assert_eq!(
            idx.offset_to_position(0),
            Some(Position {
                line: 0,
                character: 0
            })
        ); // 'a'
        assert_eq!(
            idx.offset_to_position(1),
            Some(Position {
                line: 0,
                character: 1
            })
        ); // start of 😀
        assert_eq!(
            idx.offset_to_position(5),
            Some(Position {
                line: 0,
                character: 3
            })
        ); // 'b' (1 + 2 surrogate units)
    }

    #[test]
    fn test_utf16_roundtrip_supplementary() {
        let idx = LineIndex::new("a😀b");
        // Position of 'b': line 0, character 3 (1 for 'a', 2 for surrogate pair)
        let offset = idx
            .position_to_offset(&Position {
                line: 0,
                character: 3,
            })
            .unwrap();
        assert_eq!(offset, 5); // 1 byte for 'a' + 4 bytes for '😀'
    }

    // ========================================================================
    // line_start / line_end
    // ========================================================================

    #[test]
    fn test_line_start_end() {
        let idx = LineIndex::new("abc\ndef\nghi");
        assert_eq!(idx.line_start(0), Some(0));
        assert_eq!(idx.line_end(0), Some(3));
        assert_eq!(idx.line_start(1), Some(4));
        assert_eq!(idx.line_end(1), Some(7));
        assert_eq!(idx.line_start(2), Some(8));
        assert_eq!(idx.line_end(2), Some(11));
    }

    #[test]
    fn test_line_end_crlf() {
        let idx = LineIndex::new("abc\r\ndef");
        // \r\n: line 0 ends at offset 3 (before \r)
        assert_eq!(idx.line_end(0), Some(3));
        assert_eq!(idx.line_start(1), Some(5)); // after \r\n
    }

    // ========================================================================
    // SFC-like source
    // ========================================================================

    #[test]
    fn test_sfc_source() {
        let source = "<template>\n  <div>{{ msg }}</div>\n</template>\n\n<script setup lang=\"ts\">\nimport { ref } from 'vue'\n\nconst msg = ref('hello')\n</script>\n";
        let idx = LineIndex::new(source);

        // Line 0: "<template>"
        assert_eq!(
            idx.offset_to_position(0),
            Some(Position {
                line: 0,
                character: 0
            })
        );
        // Line 5: "import { ref } from 'vue'"
        let line5_start = idx.line_start(5).unwrap();
        assert_eq!(
            idx.offset_to_position(line5_start),
            Some(Position {
                line: 5,
                character: 0
            })
        );
        // Verify the content at line 5 starts with "import"
        assert_eq!(
            &source[line5_start as usize..line5_start as usize + 6],
            "import"
        );
    }
}
