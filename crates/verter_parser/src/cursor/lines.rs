//! Line offset finder implementations for calculating line numbers from byte offsets.
//!
//! This module provides multiple implementations for finding newline positions,
//! which can be used to convert byte offsets to line numbers efficiently.

use bumpalo::Bump;
use memchr::{memchr, memchr_iter};

/// Result type containing the offsets of all newlines in the input.
/// Each offset points to the `\n` character.
pub type LineOffsets = Vec<usize>;

/// Naive byte-by-byte implementation.
/// Simple loop checking each byte for newline.
#[inline(never)]
pub fn find_lines_naive(input: &[u8]) -> LineOffsets {
    let mut offsets = Vec::new();
    for (i, &byte) in input.iter().enumerate() {
        if byte == b'\n' {
            offsets.push(i);
        }
    }
    offsets
}

/// Naive implementation with capacity pre-estimation.
/// Estimates ~40 chars per line on average.
#[inline(never)]
pub fn find_lines_naive_with_capacity(input: &[u8]) -> LineOffsets {
    let estimated_lines = input.len() / 40;
    let mut offsets = Vec::with_capacity(estimated_lines);
    for (i, &byte) in input.iter().enumerate() {
        if byte == b'\n' {
            offsets.push(i);
        }
    }
    offsets
}

/// Using memchr crate with iterator API.
/// memchr uses SIMD instructions when available.
#[inline(never)]
pub fn find_lines_memchr_iter(input: &[u8]) -> LineOffsets {
    memchr_iter(b'\n', input).collect()
}

/// Using memchr crate with iterator API and pre-allocated capacity.
#[inline(never)]
pub fn find_lines_memchr_iter_with_capacity(input: &[u8]) -> LineOffsets {
    let estimated_lines = input.len() / 40;
    let mut offsets = Vec::with_capacity(estimated_lines);
    offsets.extend(memchr_iter(b'\n', input));
    offsets
}

/// Using memchr with manual iteration (repeated memchr calls).
#[inline(never)]
pub fn find_lines_memchr_manual(input: &[u8]) -> LineOffsets {
    let mut offsets = Vec::new();
    let mut start = 0;
    while let Some(pos) = memchr(b'\n', &input[start..]) {
        let absolute_pos = start + pos;
        offsets.push(absolute_pos);
        start = absolute_pos + 1;
    }
    offsets
}

/// Using memchr with manual iteration and pre-allocated capacity.
#[inline(never)]
pub fn find_lines_memchr_manual_with_capacity(input: &[u8]) -> LineOffsets {
    let estimated_lines = input.len() / 40;
    let mut offsets = Vec::with_capacity(estimated_lines);
    let mut start = 0;
    while let Some(pos) = memchr(b'\n', &input[start..]) {
        let absolute_pos = start + pos;
        offsets.push(absolute_pos);
        start = absolute_pos + 1;
    }
    offsets
}

/// Chunk-based processing - processes 8 bytes at a time.
/// Falls back to byte-by-byte for remainder.
#[inline(never)]
pub fn find_lines_chunks(input: &[u8]) -> LineOffsets {
    let mut offsets = Vec::new();
    let chunks = input.chunks_exact(8);
    let remainder = chunks.remainder();
    let mut base_offset = 0;

    for chunk in chunks {
        // Check each byte in the chunk
        for (i, &byte) in chunk.iter().enumerate() {
            if byte == b'\n' {
                offsets.push(base_offset + i);
            }
        }
        base_offset += 8;
    }

    // Handle remainder
    for (i, &byte) in remainder.iter().enumerate() {
        if byte == b'\n' {
            offsets.push(base_offset + i);
        }
    }

    offsets
}

/// Chunk-based with u64 comparison for quick elimination.
/// Uses bit manipulation to detect newlines in 8-byte chunks.
#[inline(never)]
pub fn find_lines_chunks_u64(input: &[u8]) -> LineOffsets {
    const NEWLINE_PATTERN: u64 = 0x0a0a0a0a0a0a0a0a; // \n repeated 8 times
    const LO_MASK: u64 = 0x0101010101010101;
    const HI_MASK: u64 = 0x8080808080808080;

    let mut offsets = Vec::new();
    let mut i = 0;

    // Process 8 bytes at a time
    while i + 8 <= input.len() {
        // Safety: we've checked bounds
        let chunk = unsafe { (input.as_ptr().add(i) as *const u64).read_unaligned() };

        // XOR with newline pattern - matching bytes become 0
        let xored = chunk ^ NEWLINE_PATTERN;

        // Use the "find zero byte" trick:
        // If a byte is 0, (byte - 1) will have high bit set, and byte itself won't
        let has_zero = (xored.wrapping_sub(LO_MASK)) & !xored & HI_MASK;

        if has_zero != 0 {
            // At least one newline in this chunk, check each byte
            for j in 0..8 {
                if input[i + j] == b'\n' {
                    offsets.push(i + j);
                }
            }
        }

        i += 8;
    }

    // Handle remainder
    while i < input.len() {
        if input[i] == b'\n' {
            offsets.push(i);
        }
        i += 1;
    }

    offsets
}

/// Pointer-based iteration avoiding bounds checks.
#[inline(never)]
pub fn find_lines_ptr(input: &[u8]) -> LineOffsets {
    let mut offsets = Vec::new();
    let len = input.len();
    let ptr = input.as_ptr();

    let mut i = 0;
    while i < len {
        // Safety: i < len guarantees this is in bounds
        if unsafe { *ptr.add(i) } == b'\n' {
            offsets.push(i);
        }
        i += 1;
    }

    offsets
}

/// Using memchr with Bump allocator for arena-based allocation.
/// Returns a slice allocated in the bump arena.
#[inline(never)]
pub fn find_lines_memchr_bump<'bump>(input: &[u8], bump: &'bump Bump) -> &'bump [usize] {
    let estimated_lines = input.len() / 40;
    let mut offsets = bumpalo::collections::Vec::with_capacity_in(estimated_lines, bump);
    offsets.extend(memchr_iter(b'\n', input));
    offsets.into_bump_slice()
}

/// Using memchr with Bump allocator, returning a bumpalo Vec.
/// Useful when you need to keep the Vec for further modifications.
#[inline(never)]
pub fn find_lines_memchr_bump_vec<'bump>(
    input: &[u8],
    bump: &'bump Bump,
) -> bumpalo::collections::Vec<'bump, usize> {
    let estimated_lines = input.len() / 40;
    let mut offsets = bumpalo::collections::Vec::with_capacity_in(estimated_lines, bump);
    offsets.extend(memchr_iter(b'\n', input));
    offsets
}

/// Count-only version using memchr - useful if you only need line count.
#[inline(never)]
pub fn count_lines_memchr(input: &[u8]) -> usize {
    memchr_iter(b'\n', input).count()
}

/// Count-only naive version.
#[inline(never)]
pub fn count_lines_naive(input: &[u8]) -> usize {
    input.iter().filter(|&&b| b == b'\n').count()
}

/// Find line number for a given offset using binary search on pre-computed offsets.
#[inline]
pub fn offset_to_line(offsets: &[usize], offset: usize) -> usize {
    match offsets.binary_search(&offset) {
        Ok(line) => line + 1,  // Offset is exactly at a newline
        Err(line) => line + 1, // Offset is between newlines
    }
}

/// Find line and column for a given offset.
#[inline]
pub fn offset_to_line_col(offsets: &[usize], offset: usize) -> (usize, usize) {
    let line = match offsets.binary_search(&offset) {
        Ok(line) => line,
        Err(line) => line,
    };

    let line_start = if line == 0 { 0 } else { offsets[line - 1] + 1 };
    let col = offset - line_start;

    (line + 1, col + 1) // 1-indexed
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_INPUT: &[u8] = b"line1\nline2\nline3\n";

    fn assert_same_results(input: &[u8]) {
        let naive = find_lines_naive(input);
        let memchr_iter = find_lines_memchr_iter(input);
        let memchr_manual = find_lines_memchr_manual(input);
        let chunks = find_lines_chunks(input);
        let chunks_u64 = find_lines_chunks_u64(input);
        let ptr = find_lines_ptr(input);

        assert_eq!(naive, memchr_iter, "memchr_iter differs from naive");
        assert_eq!(naive, memchr_manual, "memchr_manual differs from naive");
        assert_eq!(naive, chunks, "chunks differs from naive");
        assert_eq!(naive, chunks_u64, "chunks_u64 differs from naive");
        assert_eq!(naive, ptr, "ptr differs from naive");
    }

    #[test]
    fn test_find_lines_basic() {
        let expected = vec![5, 11, 17];
        assert_eq!(find_lines_naive(TEST_INPUT), expected);
        assert_eq!(find_lines_memchr_iter(TEST_INPUT), expected);
        assert_eq!(find_lines_memchr_manual(TEST_INPUT), expected);
        assert_eq!(find_lines_chunks(TEST_INPUT), expected);
        assert_eq!(find_lines_chunks_u64(TEST_INPUT), expected);
        assert_eq!(find_lines_ptr(TEST_INPUT), expected);
    }

    #[test]
    fn test_find_lines_empty() {
        assert_same_results(b"");
    }

    #[test]
    fn test_find_lines_no_newlines() {
        assert_same_results(b"no newlines here");
    }

    #[test]
    fn test_find_lines_only_newlines() {
        assert_same_results(b"\n\n\n\n");
    }

    #[test]
    fn test_find_lines_single_newline() {
        assert_same_results(b"\n");
    }

    #[test]
    fn test_find_lines_long_lines() {
        let long = b"a".repeat(1000);
        let mut input = long.clone();
        input.push(b'\n');
        input.extend(&long);
        input.push(b'\n');
        assert_same_results(&input);
    }

    #[test]
    fn test_offset_to_line() {
        let offsets = find_lines_naive(TEST_INPUT);
        // "line1\nline2\nline3\n"
        //  01234 5 67890 1 23456 7

        assert_eq!(offset_to_line(&offsets, 0), 1); // 'l' in line1
        assert_eq!(offset_to_line(&offsets, 5), 1); // '\n' after line1
        assert_eq!(offset_to_line(&offsets, 6), 2); // 'l' in line2
        assert_eq!(offset_to_line(&offsets, 11), 2); // '\n' after line2
        assert_eq!(offset_to_line(&offsets, 12), 3); // 'l' in line3
    }

    #[test]
    fn test_offset_to_line_col() {
        let offsets = find_lines_naive(TEST_INPUT);

        assert_eq!(offset_to_line_col(&offsets, 0), (1, 1)); // 'l' in line1
        assert_eq!(offset_to_line_col(&offsets, 4), (1, 5)); // '1' in line1
        assert_eq!(offset_to_line_col(&offsets, 6), (2, 1)); // 'l' in line2
        assert_eq!(offset_to_line_col(&offsets, 12), (3, 1)); // 'l' in line3
    }
}
