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
// Shared conversion core
// ---------------------------------------------------------------------------

/// Borrowed line-start table plus source — the single implementation of every
/// offset ↔ position conversion in this module.
///
/// Both index types below hold exactly these three pieces and delegate here, so
/// the owning and the borrowing index can never disagree about an encoding,
/// a bound check, or a clamp.
#[derive(Debug, Clone, Copy)]
struct IndexRef<'a> {
    line_starts: &'a [u32],
    source: &'a str,
    encoding: PositionEncoding,
}

/// Scan `source` once for its line-start byte offsets. Index 0 is always present.
fn scan_line_starts(source: &str) -> Vec<u32> {
    let mut line_starts = vec![0u32];
    for (i, &b) in source.as_bytes().iter().enumerate() {
        if b == b'\n' {
            line_starts.push((i + 1) as u32);
        }
    }
    line_starts
}

impl IndexRef<'_> {
    fn offset_to_position(&self, offset: u32) -> Option<LineColumn> {
        let offset = offset as usize;
        if offset > self.source.len() {
            return None;
        }

        let line = match self.line_starts.binary_search(&(offset as u32)) {
            Ok(exact) => exact,
            Err(insert) => insert - 1,
        };

        let line_start = self.line_starts[line] as usize;
        // Byte slicing (not `str` slicing) is deliberate: a caller may hand us an
        // offset that falls inside a multi-byte character, and the width helpers
        // fall back to the byte length for a non-UTF-8 slice rather than panicking.
        let col_bytes = &self.source.as_bytes()[line_start..offset];
        Some(LineColumn {
            line: line as u32,
            character: self.column_width(col_bytes),
        })
    }

    /// Width of `bytes` in this index's position-encoding units.
    fn column_width(&self, bytes: &[u8]) -> u32 {
        match self.encoding {
            PositionEncoding::Utf8 => bytes.len() as u32,
            PositionEncoding::Utf32 => utf8_byte_len_to_utf32_len(bytes) as u32,
            PositionEncoding::Utf16 => utf8_byte_len_to_utf16_len(bytes) as u32,
        }
    }

    fn position_to_offset(&self, pos: LineColumn) -> Option<u32> {
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

        let line_bytes = &self.source.as_bytes()[line_start..line_end];
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

    fn line_start(&self, line: usize) -> Option<u32> {
        self.line_starts.get(line).copied()
    }

    fn line_end(&self, line: usize) -> Option<u32> {
        let _start = self.line_start(line)?;
        let bytes = self.source.as_bytes();
        let end = if line + 1 < self.line_starts.len() {
            let next_start = self.line_starts[line + 1] as usize;
            if next_start > 0 && bytes.get(next_start - 1) == Some(&b'\n') {
                if next_start > 1 && bytes.get(next_start - 2) == Some(&b'\r') {
                    next_start - 2
                } else {
                    next_start - 1
                }
            } else {
                next_start
            }
        } else {
            bytes.len()
        };
        Some(end as u32)
    }

    fn line_length(&self, line: usize) -> Option<u32> {
        let start = self.line_start(line)? as usize;
        let end = self.line_end(line)? as usize;
        Some(self.column_width(&self.source.as_bytes()[start..end]))
    }

    fn checked_position_to_offset(&self, pos: LineColumn) -> Option<u32> {
        if pos.line as usize >= self.line_starts.len() {
            return None; // past-EOF line
        }
        if pos.character > self.line_length(pos.line as usize)? {
            return None; // column past the line end
        }
        let offset = self.position_to_offset(pos)?;
        // A column landing between the two halves of an astral (surrogate-pair)
        // character is not a scalar boundary; `position_to_offset` rounds it to an
        // adjacent character, yielding an offset that does NOT map back to the
        // requested column. Require the round-trip to be exact.
        if self.offset_to_position(offset)? != pos {
            return None;
        }
        Some(offset)
    }
}

// ---------------------------------------------------------------------------
// SourceIndex — reusable, borrowing index
// ---------------------------------------------------------------------------

/// Line index over a BORROWED immutable source snapshot.
///
/// Owns only the line-start table (4 bytes per line) and borrows the source
/// text, so building one never copies the document. Build it ONCE per immutable
/// source snapshot or response batch and convert every endpoint through it: a
/// per-conversion index rebuild makes a D-diagnostic batch scan the same source
/// 2D times.
#[derive(Debug, Clone)]
pub struct SourceIndex<'a> {
    line_starts: Vec<u32>,
    source: &'a str,
    encoding: PositionEncoding,
}

impl<'a> SourceIndex<'a> {
    /// Scan `source` once and retain a reusable index over it.
    pub fn new(source: &'a str, encoding: PositionEncoding) -> Self {
        Self {
            line_starts: scan_line_starts(source),
            source,
            encoding,
        }
    }

    /// Reusable index with the default UTF-16 encoding (LSP / tsserver default).
    pub fn new_utf16(source: &'a str) -> Self {
        Self::new(source, PositionEncoding::Utf16)
    }

    fn as_index_ref(&self) -> IndexRef<'_> {
        IndexRef {
            line_starts: &self.line_starts,
            source: self.source,
            encoding: self.encoding,
        }
    }

    /// The borrowed source this index was built over.
    pub fn source(&self) -> &'a str {
        self.source
    }

    /// Convert a byte offset to a line/column position (0-indexed, encoding-dependent column).
    pub fn offset_to_position(&self, offset: u32) -> Option<LineColumn> {
        self.as_index_ref().offset_to_position(offset)
    }

    /// Convert a line/column position (0-indexed, encoding-dependent column) to a byte offset.
    pub fn position_to_offset(&self, pos: LineColumn) -> Option<u32> {
        self.as_index_ref().position_to_offset(pos)
    }

    /// Convert a line/column to a byte offset, clamping an out-of-range position to EOF.
    ///
    /// Fails OPEN — the navigation-sentinel convention. An EDIT or a secondary
    /// link must use [`Self::checked_position_to_offset`] instead.
    pub fn clamped_position_to_offset(&self, pos: LineColumn) -> u32 {
        self.position_to_offset(pos)
            .unwrap_or(self.source.len() as u32)
    }

    /// Convert a line/column to a byte offset, returning `None` when the position is OUT OF RANGE
    /// instead of clamping it to EOF.
    ///
    /// Fails CLOSED: a past-EOF line, a column past the line's end, or a column landing inside a
    /// surrogate pair is rejected. Edit and secondary-link paths use this, because a clamped wrong
    /// offset corrupts a file or forges a bogus "see declaration" link at EOF.
    pub fn checked_position_to_offset(&self, pos: LineColumn) -> Option<u32> {
        self.as_index_ref().checked_position_to_offset(pos)
    }

    /// Return the number of lines in the source.
    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    /// Return the byte offset of the start of a line.
    pub fn line_start(&self, line: usize) -> Option<u32> {
        self.as_index_ref().line_start(line)
    }

    /// Return the byte offset of the end of a line (before the newline, or EOF).
    pub fn line_end(&self, line: usize) -> Option<u32> {
        self.as_index_ref().line_end(line)
    }

    /// Width of a line in this index's position-encoding units, excluding the line terminator.
    pub fn line_length(&self, line: usize) -> Option<u32> {
        self.as_index_ref().line_length(line)
    }

    /// Return the total byte length of the source text.
    pub fn source_len(&self) -> u32 {
        self.source.len() as u32
    }

    /// The position encoding this index was built with.
    pub fn encoding(&self) -> PositionEncoding {
        self.encoding
    }
}

// ---------------------------------------------------------------------------
// LineIndex — owning index
// ---------------------------------------------------------------------------

/// Precomputed line start offsets for fast byte-offset ↔ line/column conversion.
///
/// Supports UTF-8, UTF-16 (default), and UTF-32 position encodings.
/// Build once per document version, then use for all position conversions.
///
/// This index OWNS a copy of the source, for a caller that outlives the buffer it
/// was built from (a stored per-document index). A caller converting positions
/// against a live buffer should use [`SourceIndex`], which borrows instead.
#[derive(Debug, Clone)]
pub struct LineIndex {
    /// Byte offset of the start of each line. `line_starts[0]` is always 0.
    line_starts: Vec<u32>,
    /// The full source text (needed for column calculation).
    source: String,
    /// Position encoding.
    encoding: PositionEncoding,
}

impl LineIndex {
    /// Build a `LineIndex` from the full source text, using the specified encoding.
    pub fn new(source: &str, encoding: PositionEncoding) -> Self {
        Self {
            line_starts: scan_line_starts(source),
            source: source.to_owned(),
            encoding,
        }
    }

    /// Build a `LineIndex` with the default UTF-16 encoding.
    pub fn new_utf16(source: &str) -> Self {
        Self::new(source, PositionEncoding::Utf16)
    }

    fn as_index_ref(&self) -> IndexRef<'_> {
        IndexRef {
            line_starts: &self.line_starts,
            source: &self.source,
            encoding: self.encoding,
        }
    }

    /// Convert a byte offset to a line/column position (0-indexed, encoding-dependent column).
    pub fn offset_to_position(&self, offset: u32) -> Option<LineColumn> {
        self.as_index_ref().offset_to_position(offset)
    }

    /// Convert a line/column position (0-indexed, encoding-dependent column) to a byte offset.
    pub fn position_to_offset(&self, pos: LineColumn) -> Option<u32> {
        self.as_index_ref().position_to_offset(pos)
    }

    /// Return the number of lines in the source.
    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    /// Return the byte offset of the start of a line.
    pub fn line_start(&self, line: usize) -> Option<u32> {
        self.as_index_ref().line_start(line)
    }

    /// Return the total byte length of the source text.
    pub fn source_len(&self) -> u32 {
        self.source.len() as u32
    }

    /// The position encoding this index was built with.
    pub fn encoding(&self) -> PositionEncoding {
        self.encoding
    }

    /// Return the byte offset of the end of a line (before the newline, or EOF).
    pub fn line_end(&self, line: usize) -> Option<u32> {
        self.as_index_ref().line_end(line)
    }
}

// ---------------------------------------------------------------------------
// Convenience functions (used by transport layers)
// ---------------------------------------------------------------------------

/// Convert a byte offset to a line/column using the given encoding.
///
/// Scans `content` on each call. A caller converting more than one position
/// against the same source should build a [`SourceIndex`] once instead.
/// Falls back to end-of-file position if offset is out of bounds.
pub fn offset_to_line_column(content: &str, offset: u32, encoding: PositionEncoding) -> LineColumn {
    let idx = SourceIndex::new(content, encoding);
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
/// Scans `content` on each call. A caller converting more than one position
/// against the same source should build a [`SourceIndex`] once instead.
/// Falls back to end of content if position is out of bounds.
pub fn line_column_to_offset(
    content: &str,
    line: u32,
    character: u32,
    encoding: PositionEncoding,
) -> u32 {
    SourceIndex::new(content, encoding).clamped_position_to_offset(LineColumn { line, character })
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

    /// A reusable index must SHARE the caller's bytes. An index that copied the
    /// source would make a D-diagnostic batch copy the document once per
    /// endpoint, which is the cost this type exists to remove.
    #[test]
    fn source_index_borrows_the_caller_buffer_instead_of_copying_it() {
        let source = String::from("const a = 1;\nconst b = 2;\n");
        let idx = SourceIndex::new_utf16(&source);
        assert_eq!(idx.source().as_ptr(), source.as_ptr());
        assert_eq!(idx.source_len(), source.len() as u32);
    }

    /// The borrowing and the owning index share one conversion implementation,
    /// so they must agree at every offset under every encoding.
    #[test]
    fn source_index_agrees_with_owning_line_index_across_encodings() {
        let source = "a😀b\ncafé\r\nxy";
        for encoding in [
            PositionEncoding::Utf8,
            PositionEncoding::Utf16,
            PositionEncoding::Utf32,
        ] {
            let owned = LineIndex::new(source, encoding);
            let borrowed = SourceIndex::new(source, encoding);
            assert_eq!(owned.line_count(), borrowed.line_count());
            for offset in 0..=source.len() as u32 {
                assert_eq!(
                    owned.offset_to_position(offset),
                    borrowed.offset_to_position(offset),
                    "offset {offset} under {encoding:?}"
                );
            }
            for line in 0..owned.line_count() {
                assert_eq!(owned.line_start(line), borrowed.line_start(line));
                assert_eq!(owned.line_end(line), borrowed.line_end(line));
                for character in 0..8 {
                    let pos = LineColumn {
                        line: line as u32,
                        character,
                    };
                    assert_eq!(
                        owned.position_to_offset(pos),
                        borrowed.position_to_offset(pos),
                        "{pos:?} under {encoding:?}"
                    );
                }
            }
        }
    }

    /// One index reused across many endpoints must produce exactly what a fresh
    /// per-endpoint index would — reuse is a work reduction, never a semantic
    /// change.
    #[test]
    fn one_reused_index_matches_per_endpoint_conversion() {
        let source = "let x = 1;\nlet ø = 'a😀b';\nlet z = 3;";
        let shared = SourceIndex::new_utf16(source);
        for offset in 0..=source.len() as u32 {
            assert_eq!(
                shared.offset_to_position(offset),
                SourceIndex::new_utf16(source).offset_to_position(offset),
            );
        }
        for line in 0..shared.line_count() as u32 {
            for character in 0..12 {
                let pos = LineColumn { line, character };
                assert_eq!(
                    shared.clamped_position_to_offset(pos),
                    line_column_to_offset_utf16(source, line, character),
                );
                assert_eq!(
                    shared.checked_position_to_offset(pos),
                    SourceIndex::new_utf16(source).checked_position_to_offset(pos),
                );
            }
        }
    }

    #[test]
    fn line_length_counts_in_the_index_encoding_and_excludes_the_terminator() {
        let source = "a😀b\ncd";
        assert_eq!(SourceIndex::new_utf16(source).line_length(0), Some(4));
        assert_eq!(
            SourceIndex::new(source, PositionEncoding::Utf8).line_length(0),
            Some(6)
        );
        assert_eq!(
            SourceIndex::new(source, PositionEncoding::Utf32).line_length(0),
            Some(3)
        );
        assert_eq!(SourceIndex::new_utf16(source).line_length(1), Some(2));
        assert_eq!(SourceIndex::new_utf16(source).line_length(2), None);
        // CRLF is a terminator, not content.
        assert_eq!(SourceIndex::new_utf16("ab\r\ncd").line_length(0), Some(2));
    }

    /// The strict converter is what edit and secondary-link paths rely on: it
    /// must reject a past-EOF line, a column past the line's end, and a column
    /// landing between the halves of a surrogate pair — never clamp them onto a
    /// valid-looking offset.
    #[test]
    fn checked_position_to_offset_rejects_out_of_range_and_surrogate_interiors() {
        let source = "a😀b";
        let idx = SourceIndex::new_utf16(source);
        let at = |line, character| idx.checked_position_to_offset(LineColumn { line, character });

        assert_eq!(at(0, 0), Some(0));
        assert_eq!(at(0, 1), Some(1));
        assert_eq!(at(0, 2), None); // inside the surrogate pair
        assert_eq!(at(0, 3), Some(5));
        assert_eq!(at(0, 4), Some(6)); // AT the line end is in range
        assert_eq!(at(0, 5), None); // past the line end
        assert_eq!(at(1, 0), None); // past-EOF line

        // The clamping converter is the fail-open counterpart: it never returns
        // `None`, which is exactly why an edit may not use it.
        assert_eq!(
            idx.clamped_position_to_offset(LineColumn {
                line: 9,
                character: 9
            }),
            source.len() as u32
        );
    }
}
