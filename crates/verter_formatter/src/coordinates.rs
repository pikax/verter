//! The formatter's two disjoint private UTF-8 byte coordinate domains.
//!
//! [`AuthoredFormatByteOffset`] / [`AuthoredFormatByteRange`] index the
//! complete authored carrier source; [`FormattedByteOffset`] /
//! [`FormattedByteRange`] index the rendered formatter output. Both are
//! half-open byte domains from the start of one exact text. They are nominal
//! types with no `From`, `Deref` or integer conversion between them and no
//! reuse of the compiler's generated-output coordinates: an authored position
//! can never stand in for a formatted one, in either direction.
//!
//! Raw integer construction is internal to the formatter: the only public
//! way an offset enters the world is [`AuthoredFormatByteOffset::for_source`]
//! (or its formatted twin), which checks the byte index against the exact
//! source text it claims to index. Every other boundary hands coordinates
//! over already typed and checked, so no public API takes a bare integer
//! where a coordinate is meant. The negative contracts below run with pinned
//! diagnostics in `tests/compile-fail/` through the workspace
//! compile-contract lane, which owns them.
//!
//! ```compile_fail,E0308
//! use verter_formatter::coordinates::{
//!     AuthoredFormatByteOffset, FormattedByteOffset,
//! };
//! fn authored_only(_offset: AuthoredFormatByteOffset) {}
//! fn formatted_offset() -> FormattedByteOffset {
//!     // However a formatted offset is obtained inside the formatter, it is
//!     // a formatted offset.
//!     unreachable!()
//! }
//! // A formatted offset cannot stand in for an authored offset.
//! authored_only(formatted_offset());
//! ```
//!
//! ```compile_fail,E0308
//! use verter_formatter::coordinates::AuthoredFormatByteOffset;
//! fn authored_only(_offset: AuthoredFormatByteOffset) {}
//! // A bare integer cannot stand in for a domain offset.
//! authored_only(0);
//! ```
//!
//! ```compile_fail,E0308
//! use verter_formatter::coordinates::{AuthoredFormatByteRange, FormattedByteOffset};
//! fn formatted_offset() -> FormattedByteOffset {
//!     unreachable!()
//! }
//! // An authored range cannot be built from formatted offsets.
//! let _ = AuthoredFormatByteRange::new(formatted_offset(), formatted_offset());
//! ```
//!
//! ```compile_fail,E0599
//! use verter_formatter::coordinates::FormattedByteOffset;
//! // Raw integer construction is not part of the public coordinate boundary.
//! let _ = FormattedByteOffset::from_raw(0);
//! ```

use std::fmt;

/// A checked construction failure for a formatter byte coordinate.
///
/// Nothing clamps, saturates or snaps: an invalid construction is refused with
/// the exact reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoordinateError {
    /// `end` precedes `start` in a half-open range.
    Inverted { start: u32, end: u32 },
    /// An offset lies past the end of the source text.
    OutOfBounds { offset: u32, source_len: usize },
    /// An offset lands inside a multi-byte UTF-8 sequence.
    NotCharBoundary { offset: u32 },
    /// The source text is longer than the offset domain can address.
    SourceTooLarge { source_len: usize },
}

impl fmt::Display for CoordinateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            CoordinateError::Inverted { start, end } => {
                write!(f, "inverted range: end {end} precedes start {start}")
            }
            CoordinateError::OutOfBounds { offset, source_len } => {
                write!(
                    f,
                    "offset {offset} is past the end of the {source_len}-byte source"
                )
            }
            CoordinateError::NotCharBoundary { offset } => {
                write!(f, "offset {offset} is inside a multi-byte UTF-8 sequence")
            }
            CoordinateError::SourceTooLarge { source_len } => {
                write!(f, "{source_len}-byte source exceeds the u32 offset domain")
            }
        }
    }
}

impl std::error::Error for CoordinateError {}

macro_rules! utf8_byte_domain {
    (
        $(#[$offset_doc:meta])*
        $offset:ident,
        $(#[$range_doc:meta])*
        $range:ident$(,)?
    ) => {
        $(#[$offset_doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $offset(u32);

        impl $offset {
            /// Construction from a raw byte value: a test-fixture seam, not a
            /// production entry point. Production code mints offsets through
            /// [`Self::for_source`], which checks the index against the exact
            /// source text. There is no `From<u32>` and no conversion into
            /// any other coordinate domain.
            #[cfg(test)]
            #[inline]
            pub(crate) const fn from_raw(value: u32) -> Self {
                Self(value)
            }

            /// Checked construction against one exact source text: rejects
            /// offsets past the end of the source, offsets inside a
            /// multi-byte UTF-8 sequence and sources longer than the offset
            /// domain can address. This is the minting boundary — the only
            /// public constructor — so a coordinate never exists unattached
            /// to the text that validates it.
            pub fn for_source(source: &str, offset: usize) -> Result<Self, CoordinateError> {
                let source_len = source.len();
                if source_len > u32::MAX as usize {
                    return Err(CoordinateError::SourceTooLarge { source_len });
                }
                if offset > source_len {
                    return Err(CoordinateError::OutOfBounds {
                        offset: offset as u32,
                        source_len,
                    });
                }
                if !source.is_char_boundary(offset) {
                    return Err(CoordinateError::NotCharBoundary {
                        offset: offset as u32,
                    });
                }
                Ok(Self(offset as u32))
            }

            /// The raw UTF-8 byte value.
            #[inline]
            pub const fn get(self) -> u32 {
                self.0
            }
        }

        $(#[$range_doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub struct $range {
            start: $offset,
            end: $offset,
        }

        impl $range {
            /// Checked half-open `[start, end)` construction. Rejects only the
            /// inverted range; bounds and UTF-8 boundaries need the exact
            /// source text and are checked by [`Self::for_source`].
            #[inline]
            pub const fn new(start: $offset, end: $offset) -> Result<Self, CoordinateError> {
                if start.get() > end.get() {
                    Err(CoordinateError::Inverted {
                        start: start.get(),
                        end: end.get(),
                    })
                } else {
                    Ok(Self { start, end })
                }
            }

            /// Checked construction against one exact source text: rejects
            /// inverted ranges, offsets past the end of the source, offsets
            /// inside a multi-byte UTF-8 sequence and sources longer than the
            /// offset domain can address. Preserves the exact byte identity of
            /// both endpoints.
            pub fn for_source(
                source: &str,
                start: $offset,
                end: $offset,
            ) -> Result<Self, CoordinateError> {
                if start.get() > end.get() {
                    return Err(CoordinateError::Inverted {
                        start: start.get(),
                        end: end.get(),
                    });
                }
                let source_len = source.len();
                if source_len > u32::MAX as usize {
                    return Err(CoordinateError::SourceTooLarge { source_len });
                }
                if end.get() as usize > source_len {
                    return Err(CoordinateError::OutOfBounds {
                        offset: end.get(),
                        source_len,
                    });
                }
                if !source.is_char_boundary(start.get() as usize) {
                    return Err(CoordinateError::NotCharBoundary {
                        offset: start.get(),
                    });
                }
                if !source.is_char_boundary(end.get() as usize) {
                    return Err(CoordinateError::NotCharBoundary { offset: end.get() });
                }
                Ok(Self { start, end })
            }

            /// The range's first byte offset.
            #[inline]
            pub const fn start(&self) -> $offset {
                self.start
            }

            /// The range's exclusive end byte offset.
            #[inline]
            pub const fn end(&self) -> $offset {
                self.end
            }

            /// The exact byte length. Checked construction guarantees
            /// `end >= start`, so this cannot underflow.
            #[inline]
            pub const fn len(&self) -> u32 {
                self.end.get() - self.start.get()
            }

            /// Whether the range covers zero bytes.
            #[inline]
            pub const fn is_empty(&self) -> bool {
                self.start.get() == self.end.get()
            }
        }
    };
}

utf8_byte_domain! {
    /// Byte offset from the start of the complete authored carrier source.
    ///
    /// The authored domain covers the entire authored extent — comments,
    /// whitespace, BOM, CRLF, holes, custom blocks and EOF included — exactly
    /// as the source artifact presents it.
    AuthoredFormatByteOffset,
    /// Half-open `[start, end)` byte range in the complete authored carrier
    /// source.
    AuthoredFormatByteRange,
}

utf8_byte_domain! {
    /// Byte offset from the start of the rendered formatter output.
    FormattedByteOffset,
    /// Half-open `[start, end)` byte range in the rendered formatter output.
    FormattedByteRange,
}

impl FormattedByteRange {
    /// Internal minting path for the renderer's final UTF-8 sink.
    pub(crate) fn for_emitted(source: &str, start: u32, end: u32) -> Result<Self, CoordinateError> {
        Self::for_source(source, FormattedByteOffset(start), FormattedByteOffset(end))
    }
}
