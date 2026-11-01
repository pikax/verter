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
//! ```compile_fail
//! use verter_formatter::coordinates::{AuthoredFormatByteOffset, FormattedByteOffset};
//! fn authored_only(_offset: AuthoredFormatByteOffset) {}
//! // A formatted offset cannot stand in for an authored offset.
//! authored_only(FormattedByteOffset::from_raw(0));
//! ```
//!
//! ```compile_fail
//! use verter_formatter::coordinates::AuthoredFormatByteOffset;
//! fn authored_only(_offset: AuthoredFormatByteOffset) {}
//! // A bare integer cannot stand in for a domain offset.
//! authored_only(0);
//! ```
//!
//! ```compile_fail
//! use verter_formatter::coordinates::{
//!     AuthoredFormatByteRange, FormattedByteOffset,
//! };
//! // An authored range cannot be built from formatted offsets.
//! let _ = AuthoredFormatByteRange::new(
//!     FormattedByteOffset::from_raw(0),
//!     FormattedByteOffset::from_raw(1),
//! );
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
            /// Explicit named construction from a raw byte value. This is the
            /// only integer entry point; there is no `From<u32>` and no
            /// conversion into any other coordinate domain.
            #[inline]
            pub const fn from_raw(value: u32) -> Self {
                Self(value)
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
