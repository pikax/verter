//! Checked egress of byte spans to a tagged wire encoding.
//!
//! Rust facts are UTF-8 byte offsets. A requester's encoding exists only at
//! the adapter that speaks it, converted once against the exact source
//! snapshot the offsets index. [`SpanConverter`] is that conversion for the
//! NAPI/WASM UTF-16 wires: it is exact or refused, so a missing snapshot, an
//! off-boundary endpoint, a past-end endpoint or an inverted range is a typed
//! [`SpanConversionError`] and never a byte pass-through, a clamp or a zero.

use std::cell::OnceCell;
use std::fmt;

use verter_protocol::types::SpanEncodingTag;
use verter_span::CoordError;

use super::offset::OffsetIndex;

/// Why a span could not be converted to its wire encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanConversionError {
    /// No source snapshot was available to convert the span against.
    SourceUnavailable,
    /// The span is not a valid range of the source snapshot.
    Coord(CoordError),
}

impl SpanConversionError {
    /// Stable wire code for this refusal.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::SourceUnavailable => "SourceUnavailable",
            Self::Coord(CoordError::InvalidBoundary) => "InvalidBoundary",
            Self::Coord(CoordError::NotCharBoundary) => "NotCharBoundary",
            Self::Coord(CoordError::Overflow) => "Overflow",
        }
    }
}

impl From<CoordError> for SpanConversionError {
    fn from(error: CoordError) -> Self {
        Self::Coord(error)
    }
}

impl fmt::Display for SpanConversionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SourceUnavailable => {
                f.write_str("no source snapshot to convert the span against")
            }
            Self::Coord(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for SpanConversionError {}

/// A diagnostic-shaped wire span: either the converted range plus its
/// encoding tag, or the typed reason it could not be produced. The diagnostic
/// itself is still reported either way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticWireSpan {
    pub start: Option<u32>,
    pub end: Option<u32>,
    pub encoding: SpanEncodingTag,
    /// [`SpanConversionError::code`] when the span could not be produced.
    pub error: Option<&'static str>,
}

/// Converts byte spans of one source into UTF-16 wire spans.
///
/// The offset index is built on the first conversion and shared by every
/// later one, so a batch of `k` spans visits the source once and a request
/// with nothing to convert never visits it.
pub struct SpanConverter<'a> {
    source: Option<&'a str>,
    index: OnceCell<OffsetIndex<'a>>,
}

impl<'a> SpanConverter<'a> {
    #[must_use]
    pub fn new(source: Option<&'a str>) -> Self {
        Self {
            source,
            index: OnceCell::new(),
        }
    }

    fn index(&self) -> Result<&OffsetIndex<'a>, SpanConversionError> {
        let source = self.source.ok_or(SpanConversionError::SourceUnavailable)?;
        Ok(self.index.get_or_init(|| OffsetIndex::new(source)))
    }

    /// UTF-8 byte offset → UTF-16 code-unit offset.
    pub fn utf16_offset(&self, byte_offset: u32) -> Result<u32, SpanConversionError> {
        Ok(self.index()?.to_utf16(byte_offset)?)
    }

    /// Half-open UTF-8 byte range → UTF-16 code-unit range.
    pub fn utf16_range(&self, start: u32, end: u32) -> Result<(u32, u32), SpanConversionError> {
        Ok(self
            .index()?
            .convert_range(start, end, super::offset::OffsetEncoding::Utf16)?)
    }

    /// The diagnostic-shaped wire span for a byte range.
    #[must_use]
    pub fn diagnostic_span(&self, start: u32, end: u32) -> DiagnosticWireSpan {
        match self.utf16_range(start, end) {
            Ok((start, end)) => DiagnosticWireSpan {
                start: Some(start),
                end: Some(end),
                encoding: SpanEncodingTag::Utf16,
                error: None,
            },
            Err(error) => DiagnosticWireSpan {
                start: None,
                end: None,
                encoding: SpanEncodingTag::Utf16,
                error: Some(error.code()),
            },
        }
    }
}
