//! A build-once, query-many UTF-16 line index over one file's content.
//!
//! The scalar converters in [`crate::tsgo_offset`] ([`utf16_offset_to_byte`] /
//! [`utf16_offset_to_line_col`]) each walk the whole content FROM THE START on
//! every call — O(content length) per offset. When a whole-program diagnostic set
//! carries many diagnostics in the same file, that is O(diagnostics × length)
//! repeated work.
//!
//! [`Utf16LineIndex`] scans the content ONCE at construction (recording each
//! line's start offset in BOTH UTF-16 code units and bytes) and then answers each
//! offset query by walking only WITHIN the containing line (bounded by the line
//! length, not the file length). It is the shared scaling substrate a per-file
//! diagnostic-source cache builds once and reuses.
//!
//! **Offset semantics are byte-identical to [`crate::tsgo_offset`]** — the same
//! TypeScript `getLineStarts` terminator set (`\n`; a LONE `\r`; U+2028; U+2029;
//! `\r\n` is ONE terminator), UTF-16 columns, past-end CLAMPS to the end, and a
//! mid-surrogate offset resolves to the surrogate pair's start. Those clamps are
//! `Ok(clamped)` (never an error) so parity with the never-erroring scalars holds;
//! [`OffsetError`] is reserved for a genuinely impossible internal state.
//!
//! [`utf16_offset_to_byte`]: crate::tsgo_offset::utf16_offset_to_byte
//! [`utf16_offset_to_line_col`]: crate::tsgo_offset::utf16_offset_to_line_col

use std::sync::Arc;

/// A 1-based `(line, col)` position, `col` in UTF-16 code units — TypeScript
/// position semantics (matching [`crate::tsgo_offset::utf16_offset_to_line_col`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineCol {
    /// 1-based line number.
    pub line: u32,
    /// 1-based column, in UTF-16 code units.
    pub col: u32,
}

/// An offset query that could not be answered. Past-end and mid-surrogate offsets
/// are NOT errors — they clamp (to preserve parity with the never-erroring scalar
/// converters). This is reserved for a genuinely impossible internal state (a
/// corrupt index whose line-start table is empty), so a caller can fail-closed
/// rather than silently mis-position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OffsetError {
    /// The index has no line starts — impossible for an index built by
    /// [`Utf16LineIndex::new`] (which always records line 1 at offset 0). Present
    /// so the query API is total and a caller never has to `unwrap` a panic path.
    EmptyIndex,
}

impl std::fmt::Display for OffsetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OffsetError::EmptyIndex => f.write_str("utf16 line index has no line starts"),
        }
    }
}

impl std::error::Error for OffsetError {}

/// Start-of-line markers, one per line, recorded during the single construction
/// scan.
#[derive(Debug, Clone, Copy)]
struct LineStart {
    /// UTF-16 code-unit offset of the line's first unit.
    u16: u32,
    /// Byte offset of the line's first byte.
    byte: u32,
}

/// A build-once UTF-16 line index over one file's content.
#[derive(Debug, Clone)]
pub struct Utf16LineIndex {
    /// The indexed content (shared; the cache stores it once).
    text: Arc<str>,
    /// One entry per line, in increasing offset order. `line_starts[0]` is always
    /// `{ u16: 0, byte: 0 }` (line 1). A terminator appends the position just AFTER
    /// it as the next line's start.
    line_starts: Vec<LineStart>,
    /// The content's total UTF-16 code-unit length — the clamp target for a
    /// past-end offset (matching the scalar's `target.min(total)`).
    total_u16: u32,
}

impl Utf16LineIndex {
    /// Build the index by scanning `text` ONCE.
    ///
    /// The terminator set matches TypeScript's `getLineStarts` exactly: `\n`; a
    /// LONE `\r` (a CR not immediately followed by LF); U+2028; U+2029. A `\r\n`
    /// pair is ONE terminator whose next line starts after the `\n`.
    pub fn new(text: impl Into<Arc<str>>) -> Self {
        let text: Arc<str> = text.into();
        // Line 1 always starts at (0, 0).
        let mut line_starts = vec![LineStart { u16: 0, byte: 0 }];

        let mut u16_idx: u32 = 0;
        let mut chars = text.char_indices().peekable();
        while let Some((byte_idx, ch)) = chars.next() {
            u16_idx += ch.len_utf16() as u32;
            let is_terminator = match ch {
                '\n' | '\u{2028}' | '\u{2029}' => true,
                // A lone CR terminates a line; the `\r` of a `\r\n` does not (its
                // `\n` is the single terminator), so peek and skip the paired case.
                '\r' => chars.peek().map(|&(_, c)| c) != Some('\n'),
                _ => false,
            };
            if is_terminator {
                // The next line starts at the position AFTER this terminator char.
                let next_byte = (byte_idx + ch.len_utf8()) as u32;
                line_starts.push(LineStart {
                    u16: u16_idx,
                    byte: next_byte,
                });
            }
        }

        Self {
            text,
            line_starts,
            total_u16: u16_idx,
        }
    }

    /// The indexed content.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The total number of lines in the indexed content (always `>= 1`).
    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    /// Convert a UTF-16 code-unit `offset` to a 1-based `(line, col)` (UTF-16
    /// column). Byte-identical to
    /// [`crate::tsgo_offset::utf16_offset_to_line_col`]: past-end clamps to the
    /// final column; a mid-surrogate offset resolves to the pair's start column.
    pub fn line_col_for_utf16(&self, offset: u32) -> Result<LineCol, OffsetError> {
        let line_idx = self.line_index_for_u16(offset)?;
        let line_start = self.line_starts[line_idx];

        // Clamp a past-end offset to the content's total UTF-16 length (final
        // column), exactly as the scalar's `target.min(u16_idx_final)` does. An
        // in-range offset (including one landing INSIDE a surrogate pair) is used
        // as-is: the scalar reports col == offset - line_start + 1 for a
        // mid-surrogate offset too (a stable in-pair column), NOT the pair-start
        // column — so no char-start snapping here.
        let eff = offset.min(self.total_u16);
        let col = eff - line_start.u16 + 1;
        Ok(LineCol {
            line: (line_idx as u32) + 1,
            col,
        })
    }

    /// Convert a UTF-16 code-unit `offset` to a BYTE offset into the content.
    /// Byte-identical to [`crate::tsgo_offset::utf16_offset_to_byte`]: past-end
    /// clamps to `text.len()`; a mid-surrogate offset resolves to the pair's start
    /// byte.
    pub fn byte_for_utf16(&self, offset: u32) -> Result<usize, OffsetError> {
        let line_idx = self.line_index_for_u16(offset)?;
        let line_start = self.line_starts[line_idx];

        // Walk chars WITHIN this line (from its byte start) counting UTF-16 units
        // until the target falls at (or strictly inside) a char — resolving to that
        // char's START byte, exactly as the scalar does but bounded by the line.
        let target = offset as usize;
        let mut u16_idx = line_start.u16 as usize;
        for (rel_byte, ch) in self.text[line_start.byte as usize..].char_indices() {
            let ch_units = ch.len_utf16();
            if target < u16_idx + ch_units {
                return Ok(line_start.byte as usize + rel_byte);
            }
            u16_idx += ch_units;
        }
        // At or past the content's total UTF-16 length ⇒ end of the byte buffer.
        Ok(self.text.len())
    }

    /// The index of the line containing `offset` (0-based into `line_starts`).
    /// A past-end offset resolves to the LAST line (the scalar's clamp), so a
    /// binary search for the greatest line start `<= offset` is correct for every
    /// offset including past-end.
    fn line_index_for_u16(&self, offset: u32) -> Result<usize, OffsetError> {
        if self.line_starts.is_empty() {
            return Err(OffsetError::EmptyIndex);
        }
        // partition_point returns the count of line starts with `u16 <= offset`;
        // subtracting one yields the containing line. Because `line_starts[0].u16
        // == 0 <= offset` always holds, the count is `>= 1` and the subtraction
        // never underflows.
        let count = self.line_starts.partition_point(|ls| ls.u16 <= offset);
        Ok(count - 1)
    }
}

#[cfg(test)]
#[path = "utf16_line_index_tests.rs"]
mod tests;
