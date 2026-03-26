//! Position codec: byte offset ↔ line/column conversion.
//!
//! Extracted from `verter_lsp::documents::line_index` with LSP-specific types
//! replaced by runtime-owned types. No tower_lsp_server dependency.
//!
//! Supports UTF-8, UTF-16 (default), and UTF-32 position encodings.

// ---------------------------------------------------------------------------
// Encoding types (replace tower_lsp_server types)
// ---------------------------------------------------------------------------

/// Position encoding kind — determines how column offsets are counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PositionEncoding {
    /// Column = byte offset within line.
    Utf8,
    /// Column = UTF-16 code unit count (handles surrogate pairs). Default for LSP.
    #[default]
    Utf16,
    /// Column = Unicode code point count.
    Utf32,
}

/// A line/column position (both 0-indexed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineColumn {
    pub line: u32,
    pub character: u32,
}

// ---------------------------------------------------------------------------
// LineIndex
// ---------------------------------------------------------------------------

/// Precomputed line start offsets for fast byte-offset ↔ line/column conversion.
///
/// Supports UTF-8, UTF-16 (default), and UTF-32 position encodings.
/// Build once per document version, then use for all position conversions.
#[derive(Debug, Clone)]
pub struct LineIndex {
    /// Byte offset of the start of each line. `line_starts[0]` is always 0.
    line_starts: Vec<u32>,
    /// The full source text (needed for column calculation).
    source: Vec<u8>,
    /// Position encoding.
    encoding: PositionEncoding,
}

impl LineIndex {
    /// Build a `LineIndex` from the full source text, using the specified encoding.
    pub fn new(source: &str, encoding: PositionEncoding) -> Self {
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
            encoding,
        }
    }

    /// Build a `LineIndex` with the default UTF-16 encoding.
    pub fn new_utf16(source: &str) -> Self {
        Self::new(source, PositionEncoding::Utf16)
    }

    /// Convert a byte offset to a line/column position (0-indexed, encoding-dependent column).
    pub fn offset_to_position(&self, offset: u32) -> Option<LineColumn> {
        let offset = offset as usize;
        if offset > self.source.len() {
            return None;
        }

        let line = match self.line_starts.binary_search(&(offset as u32)) {
            Ok(exact) => exact,
            Err(insert) => insert - 1,
        };

        let line_start = self.line_starts[line] as usize;
        let col_bytes = &self.source[line_start..offset];
        let character = match self.encoding {
            PositionEncoding::Utf8 => col_bytes.len() as u32,
            PositionEncoding::Utf32 => utf8_byte_len_to_utf32_len(col_bytes) as u32,
            PositionEncoding::Utf16 => utf8_byte_len_to_utf16_len(col_bytes) as u32,
        };

        Some(LineColumn {
            line: line as u32,
            character,
        })
    }

    /// Convert a line/column position (0-indexed, encoding-dependent column) to a byte offset.
    pub fn position_to_offset(&self, pos: LineColumn) -> Option<u32> {
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
        let byte_col = match self.encoding {
            PositionEncoding::Utf8 => (pos.character as usize).min(line_bytes.len()),
            PositionEncoding::Utf32 => utf32_col_to_byte_col(line_bytes, pos.character as usize),
            PositionEncoding::Utf16 => utf16_col_to_byte_col(line_bytes, pos.character as usize),
        };

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

    /// Return the total byte length of the source text.
    pub fn source_len(&self) -> u32 {
        self.source.len() as u32
    }

    /// Return the byte offset of the end of a line (before the newline, or EOF).
    pub fn line_end(&self, line: usize) -> Option<u32> {
        let _start = self.line_start(line)?;
        let end = if line + 1 < self.line_starts.len() {
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
        Some(end as u32)
    }
}

// ---------------------------------------------------------------------------
// Convenience functions (used by transport layers)
// ---------------------------------------------------------------------------

/// Convert a byte offset to a line/column using the given encoding.
///
/// Creates a `LineIndex` on each call (cheap — no allocation beyond Vec).
/// Falls back to end-of-file position if offset is out of bounds.
pub fn offset_to_line_column(content: &str, offset: u32, encoding: PositionEncoding) -> LineColumn {
    let idx = LineIndex::new(content, encoding);
    idx.offset_to_position(offset).unwrap_or_else(|| {
        // Fallback: position at end of file
        let last_line = idx.line_count().saturating_sub(1);
        LineColumn {
            line: last_line as u32,
            character: 0,
        }
    })
}

/// Convert a line/column to a byte offset using the given encoding.
///
/// Creates a `LineIndex` on each call (cheap — no allocation beyond Vec).
/// Falls back to end of content if position is out of bounds.
pub fn line_column_to_offset(
    content: &str,
    line: u32,
    character: u32,
    encoding: PositionEncoding,
) -> u32 {
    let idx = LineIndex::new(content, encoding);
    idx.position_to_offset(LineColumn { line, character })
        .unwrap_or(content.len() as u32)
}

/// Convert a byte offset to a line/column using UTF-16 encoding (default for tsserver/TSGO).
pub fn offset_to_line_column_utf16(content: &str, offset: u32) -> LineColumn {
    offset_to_line_column(content, offset, PositionEncoding::Utf16)
}

/// Convert a line/column to a byte offset using UTF-16 encoding (default for tsserver/TSGO).
pub fn line_column_to_offset_utf16(content: &str, line: u32, character: u32) -> u32 {
    line_column_to_offset(content, line, character, PositionEncoding::Utf16)
}

// ---------------------------------------------------------------------------
// Internal encoding helpers
// ---------------------------------------------------------------------------

fn utf8_byte_len_to_utf16_len(bytes: &[u8]) -> usize {
    let s = match std::str::from_utf8(bytes) {
        Ok(s) => s,
        Err(_) => return bytes.len(),
    };
    s.encode_utf16().count()
}

fn utf8_byte_len_to_utf32_len(bytes: &[u8]) -> usize {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.chars().count(),
        Err(_) => bytes.len(),
    }
}

fn utf16_col_to_byte_col(line_bytes: &[u8], utf16_col: usize) -> usize {
    let line_str = match std::str::from_utf8(line_bytes) {
        Ok(s) => s,
        Err(_) => return utf16_col.min(line_bytes.len()),
    };

    let mut utf16_count = 0;
    for (byte_idx, ch) in line_str.char_indices() {
        if utf16_count >= utf16_col {
            return byte_idx;
        }
        utf16_count += ch.len_utf16();
    }

    line_str.len().min(line_bytes.len())
}

fn utf32_col_to_byte_col(line_bytes: &[u8], utf32_col: usize) -> usize {
    let s = match std::str::from_utf8(line_bytes) {
        Ok(s) => s,
        Err(_) => return utf32_col.min(line_bytes.len()),
    };
    s.char_indices()
        .nth(utf32_col)
        .map(|(i, _)| i)
        .unwrap_or(s.len())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_source() {
        let idx = LineIndex::new_utf16("");
        assert_eq!(idx.line_count(), 1);
        assert_eq!(
            idx.offset_to_position(0),
            Some(LineColumn {
                line: 0,
                character: 0
            })
        );
    }

    #[test]
    fn test_single_line() {
        let idx = LineIndex::new_utf16("hello");
        assert_eq!(idx.line_count(), 1);
        assert_eq!(
            idx.offset_to_position(0),
            Some(LineColumn {
                line: 0,
                character: 0
            })
        );
        assert_eq!(
            idx.offset_to_position(5),
            Some(LineColumn {
                line: 0,
                character: 5
            })
        );
    }

    #[test]
    fn test_multiple_lines() {
        let idx = LineIndex::new_utf16("abc\ndef\nghi");
        assert_eq!(idx.line_count(), 3);
        assert_eq!(
            idx.offset_to_position(0),
            Some(LineColumn {
                line: 0,
                character: 0
            })
        );
        assert_eq!(
            idx.offset_to_position(4),
            Some(LineColumn {
                line: 1,
                character: 0
            })
        );
        assert_eq!(
            idx.offset_to_position(8),
            Some(LineColumn {
                line: 2,
                character: 0
            })
        );
        assert_eq!(
            idx.offset_to_position(5),
            Some(LineColumn {
                line: 1,
                character: 1
            })
        );
    }

    #[test]
    fn test_offset_out_of_bounds() {
        let idx = LineIndex::new_utf16("abc");
        assert!(idx.offset_to_position(4).is_none());
    }

    #[test]
    fn test_position_to_offset_basic() {
        let idx = LineIndex::new_utf16("abc\ndef\nghi");
        assert_eq!(
            idx.position_to_offset(LineColumn {
                line: 0,
                character: 0
            }),
            Some(0)
        );
        assert_eq!(
            idx.position_to_offset(LineColumn {
                line: 1,
                character: 0
            }),
            Some(4)
        );
        assert_eq!(
            idx.position_to_offset(LineColumn {
                line: 2,
                character: 2
            }),
            Some(10)
        );
    }

    #[test]
    fn test_roundtrip_ascii() {
        let source = "<script setup>\nconst x = ref(0);\nconst y = 'hello';\n</script>";
        let idx = LineIndex::new_utf16(source);
        for offset in 0..source.len() as u32 {
            let pos = idx.offset_to_position(offset).unwrap();
            let back = idx.position_to_offset(pos).unwrap();
            assert_eq!(back, offset, "roundtrip failed for offset {offset}");
        }
    }

    #[test]
    fn test_utf16_supplementary_character() {
        let idx = LineIndex::new_utf16("a😀b");
        assert_eq!(
            idx.offset_to_position(0),
            Some(LineColumn {
                line: 0,
                character: 0
            })
        );
        assert_eq!(
            idx.offset_to_position(1),
            Some(LineColumn {
                line: 0,
                character: 1
            })
        );
        assert_eq!(
            idx.offset_to_position(5),
            Some(LineColumn {
                line: 0,
                character: 3
            })
        );
    }

    #[test]
    fn test_utf16_roundtrip_supplementary() {
        let idx = LineIndex::new_utf16("a😀b");
        let offset = idx
            .position_to_offset(LineColumn {
                line: 0,
                character: 3,
            })
            .unwrap();
        assert_eq!(offset, 5);
    }

    #[test]
    fn test_utf8_encoding() {
        let idx = LineIndex::new("café", PositionEncoding::Utf8);
        assert_eq!(
            idx.offset_to_position(3),
            Some(LineColumn {
                line: 0,
                character: 3
            })
        );
        assert_eq!(
            idx.offset_to_position(5),
            Some(LineColumn {
                line: 0,
                character: 5
            })
        );
    }

    #[test]
    fn test_utf32_encoding() {
        let idx = LineIndex::new("a😀b", PositionEncoding::Utf32);
        assert_eq!(
            idx.offset_to_position(0),
            Some(LineColumn {
                line: 0,
                character: 0
            })
        );
        assert_eq!(
            idx.offset_to_position(1),
            Some(LineColumn {
                line: 0,
                character: 1
            })
        );
        assert_eq!(
            idx.offset_to_position(5),
            Some(LineColumn {
                line: 0,
                character: 2
            })
        );
    }

    #[test]
    fn test_line_start_end() {
        let idx = LineIndex::new_utf16("abc\ndef\nghi");
        assert_eq!(idx.line_start(0), Some(0));
        assert_eq!(idx.line_end(0), Some(3));
        assert_eq!(idx.line_start(1), Some(4));
        assert_eq!(idx.line_end(1), Some(7));
    }

    #[test]
    fn test_line_end_crlf() {
        let idx = LineIndex::new_utf16("abc\r\ndef");
        assert_eq!(idx.line_end(0), Some(3));
        assert_eq!(idx.line_start(1), Some(5));
    }

    #[test]
    fn test_convenience_offset_to_line_column_utf16() {
        let lc = offset_to_line_column_utf16("abc\ndef", 4);
        assert_eq!(
            lc,
            LineColumn {
                line: 1,
                character: 0
            }
        );
    }

    #[test]
    fn test_convenience_line_column_to_offset_utf16() {
        let offset = line_column_to_offset_utf16("abc\ndef", 1, 0);
        assert_eq!(offset, 4);
    }

    #[test]
    fn test_convenience_fallback_on_oob() {
        let lc = offset_to_line_column_utf16("abc", 999);
        assert_eq!(lc.line, 0); // fallback to last line
    }
}
