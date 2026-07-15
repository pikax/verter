//! Line index for byte-offset ↔ LSP Position conversion.
//!
//! Delegates to `verter_type_runtime::codec::LineIndex` for the actual computation.
//! This wrapper converts between LSP types (`Position`, `PositionEncodingKind`)
//! and runtime types (`LineColumn`, `PositionEncoding`).

use tower_lsp_server::ls_types::{Position, PositionEncodingKind};
use verter_type_runtime::codec;

/// Precomputed line start offsets for fast byte-offset ↔ LSP Position conversion.
///
/// Thin wrapper around `verter_type_runtime::codec::LineIndex` that converts
/// between LSP types and runtime types.
#[derive(Debug, Clone)]
pub struct LineIndex {
    inner: codec::LineIndex,
}

fn to_runtime_encoding(encoding: PositionEncodingKind) -> codec::PositionEncoding {
    if encoding == PositionEncodingKind::UTF8 {
        codec::PositionEncoding::Utf8
    } else if encoding == PositionEncodingKind::UTF32 {
        codec::PositionEncoding::Utf32
    } else {
        codec::PositionEncoding::Utf16
    }
}

impl LineIndex {
    /// Build a `LineIndex` from the full source text, using the negotiated encoding.
    pub fn new(source: &str, encoding: PositionEncodingKind) -> Self {
        Self {
            inner: codec::LineIndex::new(source, to_runtime_encoding(encoding)),
        }
    }

    /// Build a `LineIndex` with the default UTF-16 encoding.
    pub fn new_utf16(source: &str) -> Self {
        Self {
            inner: codec::LineIndex::new_utf16(source),
        }
    }

    /// Convert a byte offset to an LSP `Position` (0-indexed line, encoding-dependent column).
    pub fn offset_to_position(&self, offset: u32) -> Option<Position> {
        self.inner.offset_to_position(offset).map(|lc| Position {
            line: lc.line,
            character: lc.character,
        })
    }

    /// Convert an LSP `Position` to a byte offset.
    pub fn position_to_offset(&self, pos: &Position) -> Option<u32> {
        self.inner.position_to_offset(codec::LineColumn {
            line: pos.line,
            character: pos.character,
        })
    }

    /// Return the number of lines in the source.
    pub fn line_count(&self) -> usize {
        self.inner.line_count()
    }

    /// Return the byte offset of the start of a line.
    pub fn line_start(&self, line: usize) -> Option<u32> {
        self.inner.line_start(line)
    }

    /// Return the total byte length of the source text.
    pub fn source_len(&self) -> u32 {
        self.inner.source_len()
    }

    /// Return the byte offset of the end of a line (before the newline, or EOF).
    pub fn line_end(&self, line: usize) -> Option<u32> {
        self.inner.line_end(line)
    }

    /// The negotiated position encoding this index was built with.
    pub fn encoding(&self) -> PositionEncodingKind {
        match self.inner.encoding() {
            codec::PositionEncoding::Utf8 => PositionEncodingKind::UTF8,
            codec::PositionEncoding::Utf32 => PositionEncodingKind::UTF32,
            codec::PositionEncoding::Utf16 => PositionEncodingKind::UTF16,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_source() {
        let idx = LineIndex::new_utf16("");
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
    fn test_multiple_lines() {
        let idx = LineIndex::new_utf16("abc\ndef\nghi");
        assert_eq!(idx.line_count(), 3);
        assert_eq!(
            idx.offset_to_position(4),
            Some(Position {
                line: 1,
                character: 0
            })
        );
    }

    #[test]
    fn test_roundtrip_ascii() {
        let source = "<script setup>\nconst x = ref(0);\n</script>";
        let idx = LineIndex::new_utf16(source);
        for offset in 0..source.len() as u32 {
            let pos = idx.offset_to_position(offset).unwrap();
            let back = idx.position_to_offset(&pos).unwrap();
            assert_eq!(back, offset, "roundtrip failed for offset {offset}");
        }
    }

    #[test]
    fn test_utf16_supplementary() {
        let idx = LineIndex::new_utf16("a😀b");
        assert_eq!(
            idx.offset_to_position(5),
            Some(Position {
                line: 0,
                character: 3
            })
        );
    }

    #[test]
    fn test_utf8_encoding() {
        let idx = LineIndex::new("café", PositionEncodingKind::UTF8);
        assert_eq!(
            idx.offset_to_position(5),
            Some(Position {
                line: 0,
                character: 5
            })
        );
    }

    #[test]
    fn test_line_start_end() {
        let idx = LineIndex::new_utf16("abc\ndef\nghi");
        assert_eq!(idx.line_start(0), Some(0));
        assert_eq!(idx.line_end(0), Some(3));
        assert_eq!(idx.line_start(1), Some(4));
    }
}
