use memchr::memchr_iter;
use std::str;

use crate::common::SourceLocation;

// ============================================================================
// UTF-16 LENGTH CALCULATION
// ============================================================================

/// Compute UTF-16 code unit count for a valid UTF-8 string.
///
/// Uses an ASCII fast-path: if all bytes are ASCII, returns byte length directly.
/// For strings containing non-ASCII characters, falls back to byte-stepping.
#[inline]
pub fn utf16_len(slice: &str) -> usize {
    let bytes: &[u8] = slice.as_bytes();

    // Fast path: if all ASCII, length equals byte length
    if bytes.iter().all(|&b| b < 0x80) {
        return bytes.len();
    }

    // Slow path for non-ASCII: walk bytes with variable stride
    utf16_len_non_ascii(bytes)
}

/// Compute UTF-16 length for strings known to contain non-ASCII.
/// Walks byte-by-byte using UTF-8 lead byte patterns.
#[inline]
fn utf16_len_non_ascii(bytes: &[u8]) -> usize {
    let mut count = 0;
    let mut i = 0;

    while i < bytes.len() {
        let b = bytes[i];
        if b < 0x80 {
            // ASCII: 1 byte -> 1 UTF-16 unit
            count += 1;
            i += 1;
        } else if b < 0xC0 {
            // Continuation byte (shouldn't be at sequence start in valid UTF-8)
            i += 1;
        } else if b < 0xE0 {
            // 2-byte sequence (U+0080 to U+07FF): 1 UTF-16 unit
            count += 1;
            i += 2;
        } else if b < 0xF0 {
            // 3-byte sequence (U+0800 to U+FFFF): 1 UTF-16 unit
            count += 1;
            i += 3;
        } else {
            // 4-byte sequence (U+10000+): 2 UTF-16 units (surrogate pair)
            count += 2;
            i += 4;
        }
    }

    count
}

/// Find all newline offsets using memchr with Bump allocator.
pub fn find_lines_memchr_bump_vec(input: &[u8]) -> Vec<usize> {
    let estimated_lines = input.len() / 40;
    let mut offsets = Vec::with_capacity(estimated_lines);
    offsets.extend(memchr_iter(b'\n', input));
    offsets
}

/// Compute UTF-16 cumulative offsets at the start of each line.
///
/// Returns a vector where `utf16_cache[i]` is the UTF-16 offset at the start of line `i`.
/// This enables efficient conversion from byte offsets to UTF-16 offsets.
///
/// The returned vector has `line_offsets.len() + 1` elements (one per line, since
/// N newlines = N+1 lines).
pub fn find_utf16_offset_from_vec(input: &str, line_offsets: &[usize]) -> Vec<usize> {
    let bytes: &[u8] = input.as_bytes();
    // N newlines = N+1 lines, so we need N+1 entries
    let num_lines = line_offsets.len() + 1;
    let mut utf16_cache = Vec::with_capacity(num_lines);
    // Line 0 always starts at UTF-16 offset 0
    utf16_cache.push(0);

    // Fast path: if all ASCII, UTF-16 offsets equal byte offsets
    if bytes.iter().all(|&b| b < 0x80) {
        for &newline_pos in line_offsets.iter() {
            // Next line starts at byte (newline_pos + 1), which equals UTF-16 offset for ASCII
            utf16_cache.push(newline_pos + 1);
        }
        return utf16_cache;
    }

    // Slow path: compute cumulative UTF-16 lengths
    let mut running_utf16: usize = 0;
    let mut prev_byte_pos: usize = 0;

    for &newline_pos in line_offsets.iter() {
        // Add UTF-16 length of segment from prev_byte_pos to newline_pos (inclusive)
        running_utf16 += utf16_len(&input[prev_byte_pos..=newline_pos]);
        utf16_cache.push(running_utf16);
        prev_byte_pos = newline_pos + 1;
    }

    utf16_cache
}

/// Resolves byte offsets to (line, column) positions with UTF-16 column counting.
pub struct PositionResolver<'a> {
    input: &'a str,
    line_offsets: Vec<usize>,
    utf16_cache: Vec<usize>,
}

impl<'a> PositionResolver<'a> {
    pub fn new(input: &'a str) -> Self {
        let line_offsets = find_lines_memchr_bump_vec(input.as_bytes());
        let utf16_cache = find_utf16_offset_from_vec(input, &line_offsets);

        Self {
            input,
            line_offsets,
            utf16_cache,
        }
    }

    /// Create a resolver optimized for source map generation.
    ///
    /// Skips building the UTF-16 cumulative offset cache (which requires a full
    /// pass over the source + a Vec allocation). Use `offset_to_line_and_col()`
    /// instead of `offset_to_line_col()` with this constructor.
    pub fn new_for_sourcemap(input: &'a str) -> Self {
        let line_offsets = find_lines_memchr_bump_vec(input.as_bytes());
        Self {
            input,
            line_offsets,
            utf16_cache: Vec::new(),
        }
    }

    /// Returns (1-based line, 1-based UTF-16 column) without computing the
    /// cumulative UTF-16 offset. More efficient for source map generation
    /// where only line/column is needed.
    #[inline]
    pub fn offset_to_line_and_col(&self, offset: usize) -> (usize, usize) {
        debug_assert!(offset <= self.input.len(), "offset out of bounds");
        debug_assert!(
            self.input.is_char_boundary(offset),
            "offset must land on a char boundary"
        );

        let line = self
            .line_offsets
            .binary_search(&offset)
            .unwrap_or_else(|i| i);

        let line_start = if line == 0 {
            0
        } else {
            self.line_offsets[line - 1] + 1
        };

        let col_utf16 = utf16_len(&self.input[line_start..offset]);
        (line + 1, col_utf16 + 1)
    }

    /// Returns the 1-based line and UTF-16 column for the given byte offset.
    #[inline]
    pub fn offset_to_line_col(&self, offset: usize) -> (usize, usize, usize) {
        debug_assert!(offset <= self.input.len(), "offset out of bounds");
        debug_assert!(
            self.input.is_char_boundary(offset),
            "offset must land on a char boundary"
        );

        // Binary search returns the line index (0-based)
        // Ok = offset is exactly at a newline, Err = offset is within a line
        let line = self
            .line_offsets
            .binary_search(&offset)
            .unwrap_or_else(|i| i);

        // Line start is either 0 (first line) or byte after previous newline
        let line_start = if line == 0 {
            0
        } else {
            self.line_offsets[line - 1] + 1
        };

        // Calculate UTF-16 column for the slice from line start to offset
        let col_utf16 = utf16_len(&self.input[line_start..offset]);
        let offset_utf16 = self.utf16_cache.get(line).copied().unwrap_or(0) + col_utf16;

        (line + 1, col_utf16 + 1, offset_utf16)
    }

    pub fn to_source_location(&self, start: u32, end: u32) -> SourceLocation<'a> {
        let (start_line, start_col, start_offset_utf16) = self.offset_to_line_col(start as usize);
        let (end_line, end_col, end_offset_utf16) = self.offset_to_line_col(end as usize);

        SourceLocation {
            start: crate::common::Position {
                // TODO offset is not utf16 aware
                offset: start,
                offset_utf16: start_offset_utf16 as u32,
                line: start_line as u32,
                column: start_col as u32,
            },
            end: crate::common::Position {
                // TODO offset is not utf16 aware
                offset: end,
                offset_utf16: end_offset_utf16 as u32,
                line: end_line as u32,
                column: end_col as u32,
            },
            source: &self.input[start as usize..end as usize],
        }
    }
    pub fn to_position(&self, offset: u32) -> crate::common::Position {
        let (line, column, offset_utf16) = self.offset_to_line_col(offset as usize);
        crate::common::Position {
            offset,

            offset_utf16: offset_utf16 as u32,
            line: line as u32,
            column: column as u32,
        }
    }

    pub fn slice(&self, start: usize, end: usize) -> &'a str {
        &self.input[start..end]
    }

    pub fn slice_bytes(&self, start: usize, end: usize) -> &'a [u8] {
        &self.input.as_bytes()[start..end]
    }
}

// ============================================================================
// LINEAR SWEEP RESOLVER
// ============================================================================

/// A linear sweep position resolver for monotonically increasing byte offsets.
///
/// Replaces O(log N) binary search with O(1) amortized lookup when offsets
/// are queried in non-decreasing order. Useful for scenarios with very large
/// line counts (10K+) where binary search overhead becomes measurable.
///
/// NOTE: Benchmarked for source map generation but NOT used there — changing
/// `emit_mapped_content`'s function signature to thread sweep state caused
/// LLVM optimization regressions (+8.7% on unmodified files). Binary search
/// on typical file sizes (~700 lines ≈ 10 comparisons) is already fast enough.
/// See `.claude/performance-guide.md` § "Opt D" for details.
pub struct PositionSweep<'a> {
    resolver: &'a PositionResolver<'a>,
    /// Current line index (0-based), tracks our position in line_offsets
    current_line: usize,
}

impl<'a> PositionSweep<'a> {
    pub fn new(resolver: &'a PositionResolver<'a>) -> Self {
        Self {
            resolver,
            current_line: 0,
        }
    }

    /// Returns (1-based line, 1-based UTF-16 column, UTF-16 offset).
    ///
    /// Assumes offsets are called in monotonically non-decreasing order.
    /// Advances linearly from the last known line position.
    #[inline]
    pub fn offset_to_line_col(&mut self, offset: usize) -> (usize, usize, usize) {
        debug_assert!(offset <= self.resolver.input.len(), "offset out of bounds");
        debug_assert!(
            self.resolver.input.is_char_boundary(offset),
            "offset must land on a char boundary"
        );

        // Advance current_line forward while offset is past the current newline
        let line_offsets = &self.resolver.line_offsets;
        while self.current_line < line_offsets.len() && offset > line_offsets[self.current_line] {
            self.current_line += 1;
        }

        let line = self.current_line;

        // Line start is either 0 (first line) or byte after previous newline
        let line_start = if line == 0 {
            0
        } else {
            line_offsets[line - 1] + 1
        };

        // Calculate UTF-16 column for the slice from line start to offset
        let col_utf16 = utf16_len(&self.resolver.input[line_start..offset]);
        let offset_utf16 = self.resolver.utf16_cache.get(line).copied().unwrap_or(0) + col_utf16;

        (line + 1, col_utf16 + 1, offset_utf16)
    }
}

impl<'a> PositionResolver<'a> {
    /// Create a linear sweep resolver for monotonically increasing offsets.
    pub fn sweep(&'a self) -> PositionSweep<'a> {
        PositionSweep::new(self)
    }
}

#[cfg(test)]
#[path = "position_tests.rs"]
mod position_tests;
