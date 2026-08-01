//! UTF-8 ↔ UTF-16 / UTF-32 offset conversions and destructured-binding span
//! translation between encodings used at the FFI boundary.

use crate::types::*;

pub(super) fn clamp_to_char_boundary(source: &str, byte_offset: usize) -> usize {
    let mut clamped = byte_offset.min(source.len());
    while clamped > 0 && !source.is_char_boundary(clamped) {
        clamped -= 1;
    }
    clamped
}

pub fn byte_offset_to_utf16(source: &str, byte_offset: u32) -> u32 {
    let clamped = clamp_to_char_boundary(source, byte_offset as usize);
    let prefix = &source.as_bytes()[..clamped];
    // A pure-ASCII prefix has one UTF-16 unit per byte. `is_ascii` is a
    // vectorised byte scan; the fallback decodes and counts per character.
    if prefix.is_ascii() {
        return clamped as u32;
    }
    source[..clamped].encode_utf16().count() as u32
}

/// One-pass offset-conversion index over a single source string.
///
/// A byte offset's UTF-16 (or UTF-32) offset is the byte offset minus the
/// *deficit* every preceding multi-byte character contributes — `len_utf8 -
/// len_utf16` per character for the UTF-16 axis, `len_utf8 - 1` for the
/// UTF-32 axis. Indexing those deficits once per source turns a batch of `k`
/// conversions from O(k · offset) prefix rescans into O(source) + O(k · log
/// m), where `m` counts only the source's non-ASCII characters.
///
/// A pure-ASCII source indexes nothing at all: every conversion is the
/// clamped byte offset. Build the index once per source and reuse it for
/// every span; a lone conversion is better served by
/// [`byte_offset_to_utf16`].
pub struct OffsetIndex<'a> {
    source: &'a str,
    /// One entry per non-ASCII character, in ascending byte order. Empty for
    /// a pure-ASCII source.
    marks: Vec<OffsetMark>,
}

#[derive(Clone, Copy)]
struct OffsetMark {
    /// Byte offset immediately past this character.
    end: u32,
    /// Cumulative `len_utf8 - len_utf16` through this character.
    utf16_deficit: u32,
    /// Cumulative `len_utf8 - 1` through this character.
    utf32_deficit: u32,
}

impl<'a> OffsetIndex<'a> {
    #[must_use]
    pub fn new(source: &'a str) -> Self {
        if source.is_ascii() {
            return Self {
                source,
                marks: Vec::new(),
            };
        }
        let mut marks = Vec::new();
        let mut utf16_deficit = 0u32;
        let mut utf32_deficit = 0u32;
        for (byte_index, ch) in source.char_indices() {
            let len_utf8 = ch.len_utf8();
            if len_utf8 == 1 {
                continue;
            }
            utf16_deficit += (len_utf8 - ch.len_utf16()) as u32;
            utf32_deficit += (len_utf8 - 1) as u32;
            marks.push(OffsetMark {
                end: (byte_index + len_utf8) as u32,
                utf16_deficit,
                utf32_deficit,
            });
        }
        Self { source, marks }
    }

    /// Deficits accumulated by every character that ends at or before
    /// `clamped`. `clamped` is always a character boundary, so no character
    /// straddles it.
    fn deficits_before(&self, clamped: u32) -> (u32, u32) {
        let settled = self.marks.partition_point(|mark| mark.end <= clamped);
        match settled.checked_sub(1).map(|last| self.marks[last]) {
            Some(mark) => (mark.utf16_deficit, mark.utf32_deficit),
            None => (0, 0),
        }
    }

    /// UTF-8 byte offset → UTF-16 code-unit offset.
    #[must_use]
    pub fn to_utf16(&self, byte_offset: u32) -> u32 {
        let clamped = clamp_to_char_boundary(self.source, byte_offset as usize) as u32;
        if self.marks.is_empty() {
            return clamped;
        }
        clamped - self.deficits_before(clamped).0
    }

    /// UTF-8 byte offset → UTF-32 (Unicode scalar) offset.
    #[must_use]
    pub fn to_utf32(&self, byte_offset: u32) -> u32 {
        let clamped = clamp_to_char_boundary(self.source, byte_offset as usize) as u32;
        if self.marks.is_empty() {
            return clamped;
        }
        clamped - self.deficits_before(clamped).1
    }

    /// UTF-8 byte offset → the target encoding's offset.
    #[must_use]
    pub fn convert(&self, byte_offset: u32, encoding: OffsetEncoding) -> u32 {
        match encoding {
            OffsetEncoding::Utf8 => byte_offset,
            OffsetEncoding::Utf16 => self.to_utf16(byte_offset),
            OffsetEncoding::Utf32 => self.to_utf32(byte_offset),
        }
    }
}

pub fn utf16_to_byte_offset(source: &str, utf16_offset: u32) -> u32 {
    let mut utf16_count = 0u32;
    for (byte_idx, ch) in source.char_indices() {
        let next = utf16_count + ch.len_utf16() as u32;
        if utf16_offset <= utf16_count || utf16_offset < next {
            return byte_idx as u32;
        }
        utf16_count = next;
    }
    source.len() as u32
}

pub(super) fn maybe_utf16_offset(raw: Option<u32>, source: Option<&str>) -> Option<u32> {
    raw.map(|offset| {
        source
            .map(|s| byte_offset_to_utf16(s, offset))
            .unwrap_or(offset)
    })
}

// ── Offset encoding conversion ──────────────────────────────────

/// Target encoding for offset conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OffsetEncoding {
    /// UTF-8 byte offsets (no conversion needed).
    Utf8,
    /// UTF-16 code units (JavaScript, default LSP).
    Utf16,
    /// Unicode scalar values (codepoints).
    Utf32,
}

/// Convert a UTF-8 byte offset to the target encoding's offset.
///
/// The `text` must be the string the byte offset refers to (either SFC source
/// or generated TSX). The function counts encoding units from the start of
/// `text` up to `byte_offset`.
pub fn convert_offset(text: &str, byte_offset: u32, encoding: OffsetEncoding) -> u32 {
    match encoding {
        OffsetEncoding::Utf8 => byte_offset,
        OffsetEncoding::Utf16 => byte_offset_to_utf16(text, byte_offset),
        OffsetEncoding::Utf32 => utf8_to_utf32_offset(text, byte_offset),
    }
}

/// Convert a UTF-8 byte offset to UTF-16 code unit offset.
pub fn utf8_to_utf16_offset(text: &str, byte_offset: u32) -> u32 {
    byte_offset_to_utf16(text, byte_offset)
}

/// Convert a UTF-8 byte offset to UTF-32 (codepoint) offset.
pub(super) fn utf8_to_utf32_offset(text: &str, byte_offset: u32) -> u32 {
    let clamped = clamp_to_char_boundary(text, byte_offset as usize);
    let prefix = &text.as_bytes()[..clamped];
    if prefix.is_ascii() {
        return clamped as u32;
    }
    text[..clamped].chars().count() as u32
}

/// Input for a single binding's source span conversion.
pub struct DestructuredBindingInput<'a> {
    pub name: &'a str,
    pub source_start: u32,
    pub source_end: u32,
}

/// Convert destructured block metadata from UTF-8 to the target encoding.
///
/// `sfc_source` is the original SFC text (for converting source spans).
/// `tsx_code` is the generated TSX text (for converting block_start/block_end).
pub fn convert_destructured_block_meta(
    bindings: &[DestructuredBindingInput<'_>],
    block_start: u32,
    block_end: u32,
    sfc_source: &str,
    tsx_code: &str,
    encoding: OffsetEncoding,
) -> FfiDestructuredBlockMeta {
    FfiDestructuredBlockMeta {
        bindings: bindings
            .iter()
            .map(|b| FfiDestructuredBinding {
                name: b.name.to_string(),
                source_start: convert_offset(sfc_source, b.source_start, encoding),
                source_end: convert_offset(sfc_source, b.source_end, encoding),
            })
            .collect(),
        block_start: convert_offset(tsx_code, block_start, encoding),
        block_end: convert_offset(tsx_code, block_end, encoding),
    }
}
