//! UTF-8 ↔ UTF-16 / UTF-32 offset conversions and destructured-binding span
//! translation between encodings used at the FFI boundary.

use super::error::FfiConversionError;
use super::wire_span::SpanConversionError;
use crate::types::*;
use std::marker::PhantomData;
use verter_span::{
    CoordError, GeneratedByteOffset, GeneratedByteRange, SourceByteOffset, SourceByteRange,
};

mod sealed {
    pub trait Sealed {}
}

/// The coordinate domain an [`OffsetIndex`] indexes: authored source bytes
/// ([`SourceDomain`]) or generated-surface bytes
/// ([`GeneratedDomain`]). An index of one domain accepts only that domain's
/// newtypes, so a source range cannot be converted against a generated
/// surface, or the reverse, by passing raw integers in the wrong order.
pub trait IndexDomain: sealed::Sealed {
    /// The domain's byte-offset newtype.
    type Offset: Copy;
    /// The domain's half-open byte-range newtype.
    type Range: Copy;
    #[doc(hidden)]
    fn offset(offset: Self::Offset) -> u32;
    #[doc(hidden)]
    fn range(range: Self::Range) -> (u32, u32);
}

/// Authored source bytes.
pub struct SourceDomain;
/// Generated-surface bytes.
pub struct GeneratedDomain;

impl sealed::Sealed for SourceDomain {}
impl sealed::Sealed for GeneratedDomain {}

impl IndexDomain for SourceDomain {
    type Offset = SourceByteOffset;
    type Range = SourceByteRange;
    fn offset(offset: SourceByteOffset) -> u32 {
        offset.get()
    }
    fn range(range: SourceByteRange) -> (u32, u32) {
        (range.start().get(), range.end().get())
    }
}

impl IndexDomain for GeneratedDomain {
    type Offset = GeneratedByteOffset;
    type Range = GeneratedByteRange;
    fn offset(offset: GeneratedByteOffset) -> u32 {
        offset.get()
    }
    fn range(range: GeneratedByteRange) -> (u32, u32) {
        (range.start().get(), range.end().get())
    }
}

/// A native source span as a checked source-domain range; an inverted span is
/// [`CoordError::InvalidBoundary`], never swapped or emptied.
pub fn source_range(span: verter_span::Span) -> Result<SourceByteRange, CoordError> {
    SourceByteRange::checked_new(
        SourceByteOffset::new(span.start),
        SourceByteOffset::new(span.end),
    )
}

/// A native generated-surface span as a checked generated-domain range; an
/// inverted span is [`CoordError::InvalidBoundary`].
pub fn generated_range(span: verter_span::Span) -> Result<GeneratedByteRange, CoordError> {
    GeneratedByteRange::checked_new(
        GeneratedByteOffset::new(span.start),
        GeneratedByteOffset::new(span.end),
    )
}

/// Source bytes visited by offset conversion on the current thread.
///
/// OPTIONAL measurement counter: it has no production consumer and exists so
/// tests and measurement builds can show that a batch of conversions visits
/// its source once rather than once per span. Default builds compile every
/// recording site away.
#[cfg(any(test, feature = "semantic-observe"))]
pub mod visit_counter {
    use std::cell::Cell;

    thread_local! {
        static SOURCE_UNITS_VISITED: Cell<u64> = const { Cell::new(0) };
    }

    pub(super) fn record(units: usize) {
        SOURCE_UNITS_VISITED.with(|visited| visited.set(visited.get() + units as u64));
    }

    /// Source bytes visited on this thread since the last [`reset`].
    #[must_use]
    pub fn source_units_visited() -> u64 {
        SOURCE_UNITS_VISITED.with(Cell::get)
    }

    /// Start a new measurement window on this thread.
    pub fn reset() {
        SOURCE_UNITS_VISITED.with(|visited| visited.set(0));
    }
}

#[cfg(any(test, feature = "semantic-observe"))]
use visit_counter::record as record_source_visit;

#[cfg(not(any(test, feature = "semantic-observe")))]
#[inline(always)]
fn record_source_visit(_units: usize) {}

/// Validate that `byte_offset` names a position inside `source`: on a UTF-8
/// character boundary and no further than end-of-file. Never snaps and never
/// clamps (a conversion is exact or refused).
fn checked_boundary(source: &str, byte_offset: u32) -> Result<usize, CoordError> {
    let offset = byte_offset as usize;
    if offset > source.len() {
        return Err(CoordError::InvalidBoundary);
    }
    if !source.is_char_boundary(offset) {
        return Err(CoordError::NotCharBoundary);
    }
    Ok(offset)
}

/// UTF-8 byte offset → UTF-16 code-unit offset against `source`, exact or
/// refused: past-end is [`CoordError::InvalidBoundary`], inside a UTF-8
/// sequence is [`CoordError::NotCharBoundary`].
pub fn byte_offset_to_utf16(source: &str, byte_offset: u32) -> Result<u32, CoordError> {
    let checked = checked_boundary(source, byte_offset)?;
    let prefix = &source.as_bytes()[..checked];
    // A pure-ASCII prefix has one UTF-16 unit per byte. `is_ascii` is a
    // vectorised byte scan; the fallback decodes and counts per character.
    record_source_visit(checked);
    if prefix.is_ascii() {
        return Ok(byte_offset);
    }
    record_source_visit(checked);
    u32::try_from(source[..checked].encode_utf16().count()).map_err(|_| CoordError::Overflow)
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
pub struct OffsetIndex<'a, D: IndexDomain = SourceDomain> {
    source: &'a str,
    /// One entry per non-ASCII character, in ascending byte order. Empty for
    /// a pure-ASCII source.
    marks: Vec<OffsetMark>,
    domain: PhantomData<fn() -> D>,
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

impl<'a> OffsetIndex<'a, SourceDomain> {
    /// Index an authored source.
    #[must_use]
    pub fn new(source: &'a str) -> Self {
        Self::build(source)
    }
}

impl<'a> OffsetIndex<'a, GeneratedDomain> {
    /// Index a generated surface.
    #[must_use]
    pub fn new_generated(source: &'a str) -> Self {
        Self::build(source)
    }
}

impl<'a, D: IndexDomain> OffsetIndex<'a, D> {
    fn build(source: &'a str) -> Self {
        record_source_visit(source.len());
        if source.is_ascii() {
            return Self {
                source,
                marks: Vec::new(),
                domain: PhantomData,
            };
        }
        record_source_visit(source.len());
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
        Self {
            source,
            marks,
            domain: PhantomData,
        }
    }

    /// Deficits accumulated by every character that ends at or before
    /// `boundary`. `boundary` has been validated as a character boundary, so
    /// no character straddles it.
    fn deficits_before(&self, boundary: u32) -> (u32, u32) {
        let settled = self.marks.partition_point(|mark| mark.end <= boundary);
        match settled.checked_sub(1).map(|last| self.marks[last]) {
            Some(mark) => (mark.utf16_deficit, mark.utf32_deficit),
            None => (0, 0),
        }
    }

    /// UTF-8 byte offset → UTF-16 code-unit offset, exact or refused.
    pub fn to_utf16(&self, offset: D::Offset) -> Result<u32, CoordError> {
        let byte_offset = D::offset(offset);
        checked_boundary(self.source, byte_offset)?;
        if self.marks.is_empty() {
            return Ok(byte_offset);
        }
        Ok(byte_offset - self.deficits_before(byte_offset).0)
    }

    /// UTF-8 byte offset → UTF-32 (Unicode scalar) offset, exact or refused.
    pub fn to_utf32(&self, offset: D::Offset) -> Result<u32, CoordError> {
        let byte_offset = D::offset(offset);
        checked_boundary(self.source, byte_offset)?;
        if self.marks.is_empty() {
            return Ok(byte_offset);
        }
        Ok(byte_offset - self.deficits_before(byte_offset).1)
    }

    /// UTF-8 byte offset → the target encoding's offset, exact or refused.
    pub fn convert(&self, offset: D::Offset, encoding: OffsetEncoding) -> Result<u32, CoordError> {
        match encoding {
            OffsetEncoding::Utf8 => {
                let byte_offset = D::offset(offset);
                checked_boundary(self.source, byte_offset)?;
                Ok(byte_offset)
            }
            OffsetEncoding::Utf16 => self.to_utf16(offset),
            OffsetEncoding::Utf32 => self.to_utf32(offset),
        }
    }

    /// Half-open byte range → the target encoding's range. Both endpoints are
    /// validated against the source; the range newtype cannot be inverted, so
    /// an inverted native span is refused where it becomes a range
    /// ([`source_range`], [`generated_range`]).
    pub fn convert_range(
        &self,
        range: D::Range,
        encoding: OffsetEncoding,
    ) -> Result<(u32, u32), CoordError> {
        let (start, end) = D::range(range);
        Ok((
            self.convert_raw(start, encoding)?,
            self.convert_raw(end, encoding)?,
        ))
    }

    fn convert_raw(&self, byte_offset: u32, encoding: OffsetEncoding) -> Result<u32, CoordError> {
        match encoding {
            OffsetEncoding::Utf8 => {
                checked_boundary(self.source, byte_offset)?;
                Ok(byte_offset)
            }
            OffsetEncoding::Utf16 | OffsetEncoding::Utf32 => {
                checked_boundary(self.source, byte_offset)?;
                if self.marks.is_empty() {
                    return Ok(byte_offset);
                }
                let (utf16, utf32) = self.deficits_before(byte_offset);
                let deficit = if encoding == OffsetEncoding::Utf16 {
                    utf16
                } else {
                    utf32
                };
                Ok(byte_offset - deficit)
            }
        }
    }
}

/// Requester UTF-16 code-unit offset → UTF-8 byte offset, exact or refused.
///
/// `source` must be the exact content the requester's offset indexes. An
/// offset inside a surrogate pair is [`CoordError::NotCharBoundary`]; one
/// past the source's UTF-16 length is [`CoordError::InvalidBoundary`]; a
/// byte position the `u32` coordinate cannot name is [`CoordError::Overflow`].
/// Nothing snaps to the scalar start or clamps to end-of-file.
pub fn utf16_to_byte_offset(source: &str, utf16_offset: u32) -> Result<u32, CoordError> {
    let mut utf16_count = 0u64;
    let target = u64::from(utf16_offset);
    for (byte_idx, ch) in source.char_indices() {
        if target == utf16_count {
            return u32::try_from(byte_idx).map_err(|_| CoordError::Overflow);
        }
        let next = utf16_count + ch.len_utf16() as u64;
        if target < next {
            return Err(CoordError::NotCharBoundary);
        }
        utf16_count = next;
    }
    if target == utf16_count {
        return u32::try_from(source.len()).map_err(|_| CoordError::Overflow);
    }
    Err(CoordError::InvalidBoundary)
}

/// Parse the versioned encoding tag a request declares for its offsets. An
/// unknown, unversioned or future-version tag is refused — a request position
/// is never interpreted under a guessed encoding.
pub fn parse_request_encoding_tag(tag: &str) -> Result<SpanEncodingTag, FfiConversionError> {
    SpanEncodingTag::from_wire(tag)
        .ok_or_else(|| FfiConversionError::InvalidEncodingTag(tag.to_string()))
}

/// Requester offset (in the encoding its request tag declares) → UTF-8 byte
/// offset of `source`, converted once, exact or refused: an offset inside a
/// UTF-16 surrogate pair or UTF-8 sequence is
/// [`FfiConversionError::InvalidPosition`]`("NotCharBoundary")`, one past the
/// end is `InvalidPosition("InvalidBoundary")`.
pub fn request_offset_to_byte(
    source: &str,
    offset: u32,
    encoding: SpanEncodingTag,
) -> Result<u32, FfiConversionError> {
    let converted = match encoding {
        SpanEncodingTag::Utf16 => utf16_to_byte_offset(source, offset),
        SpanEncodingTag::Utf8 => checked_boundary(source, offset).map(|_| offset),
    };
    converted.map_err(|error| {
        FfiConversionError::InvalidPosition(SpanConversionError::from(error).code())
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

/// Convert a UTF-8 byte offset to the target encoding's offset, exact or
/// refused.
///
/// The `text` must be the string the byte offset refers to (either SFC source
/// or generated TSX). The function counts encoding units from the start of
/// `text` up to `byte_offset`.
pub fn convert_offset(
    text: &str,
    byte_offset: u32,
    encoding: OffsetEncoding,
) -> Result<u32, CoordError> {
    match encoding {
        OffsetEncoding::Utf8 => checked_boundary(text, byte_offset).map(|_| byte_offset),
        OffsetEncoding::Utf16 => byte_offset_to_utf16(text, byte_offset),
        OffsetEncoding::Utf32 => utf8_to_utf32_offset(text, byte_offset),
    }
}

/// Convert a UTF-8 byte offset to UTF-16 code unit offset, exact or refused.
pub fn utf8_to_utf16_offset(text: &str, byte_offset: u32) -> Result<u32, CoordError> {
    byte_offset_to_utf16(text, byte_offset)
}

/// Convert a UTF-8 byte offset to UTF-32 (codepoint) offset, exact or refused.
pub(super) fn utf8_to_utf32_offset(text: &str, byte_offset: u32) -> Result<u32, CoordError> {
    let checked = checked_boundary(text, byte_offset)?;
    let prefix = &text.as_bytes()[..checked];
    record_source_visit(checked);
    if prefix.is_ascii() {
        return Ok(byte_offset);
    }
    record_source_visit(checked);
    u32::try_from(text[..checked].chars().count()).map_err(|_| CoordError::Overflow)
}

/// Input for a single binding's source span conversion.
pub struct DestructuredBindingInput<'a> {
    pub name: &'a str,
    /// The binding's authored-source range.
    pub source: SourceByteRange,
}

/// Convert destructured block metadata from UTF-8 to the tagged wire encoding.
///
/// `sfc_source` is the original SFC text the binding spans index; it is
/// required whenever there is a binding to convert, and its absence is
/// [`SpanConversionError::SourceUnavailable`] — never zero offsets.
/// `tsx_code` is the generated TSX the block bounds index (a different
/// coordinate domain: each field pair names its own encoding). Every endpoint
/// is validated against its snapshot; an off-boundary, past-end or inverted
/// range refuses the whole conversion.
pub fn convert_destructured_block_meta(
    bindings: &[DestructuredBindingInput<'_>],
    block: GeneratedByteRange,
    sfc_source: Option<&str>,
    tsx_code: &str,
    encoding: SpanEncodingTag,
) -> Result<FfiDestructuredBlockMeta, SpanConversionError> {
    let offset_encoding = match encoding {
        SpanEncodingTag::Utf8 => OffsetEncoding::Utf8,
        SpanEncodingTag::Utf16 => OffsetEncoding::Utf16,
    };
    let tsx_index = OffsetIndex::new_generated(tsx_code);
    let (block_start, block_end) = tsx_index.convert_range(block, offset_encoding)?;
    // One index per source, built only when there is a binding to convert.
    let sfc_index = if bindings.is_empty() {
        None
    } else {
        Some(OffsetIndex::new(
            sfc_source.ok_or(SpanConversionError::SourceUnavailable)?,
        ))
    };
    let bindings = bindings
        .iter()
        .map(|b| {
            let index = sfc_index
                .as_ref()
                .ok_or(SpanConversionError::SourceUnavailable)?;
            let (source_start, source_end) = index.convert_range(b.source, offset_encoding)?;
            Ok(FfiDestructuredBinding {
                name: b.name.to_string(),
                source_start,
                source_end,
                source_encoding: encoding,
            })
        })
        .collect::<Result<Vec<_>, SpanConversionError>>()?;
    Ok(FfiDestructuredBlockMeta {
        bindings,
        block_start,
        block_end,
        block_encoding: encoding,
    })
}
