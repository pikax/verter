//! Requester-encoding converters, test-support only.
//!
//! UTF-16/UTF-32 code-unit and TypeScript-wire line/column conversion belongs
//! to the boundary owners (the language-intelligence and framework adapters),
//! which own the wire base, the terminator set and the exact snapshot they
//! convert against (constitution `R03`, `R10`). The core coordinate domains
//! expose no requester-encoding converter in production; these helpers exist
//! so the case table (K13–K25) can pin the exact-or-refused conversion
//! contract against an executable model, and they are not an API.

use super::{CoordError, EmbeddedByteOffset, GeneratedByteOffset, SourceByteOffset};
use crate::utf16_line_index::LineCol;

/// Narrow a byte position (or any counted extent) to the coordinate width,
/// refusing — never truncating — past `u32::MAX` (`R06`: exact or refused).
/// The checked converters mint no coordinate they cannot name; the counters
/// they narrow are kept in `u64`, where a realizable content's totals
/// cannot wrap, so this is the only narrowing step.
#[inline]
pub(super) const fn in_u32(count: u64) -> Result<u32, CoordError> {
    if count > u32::MAX as u64 {
        Err(CoordError::Overflow)
    } else {
        Ok(count as u32)
    }
}

/// Build the 1-based wire form from the `u64` counters a converter walked,
/// refusing a line or column the `u32` wire form cannot carry
/// ([`CoordError::Overflow`]).
#[inline]
pub(super) fn wire_line_col(line: u64, into_line_units: u64) -> Result<LineCol, CoordError> {
    Ok(LineCol {
        line: in_u32(line)?,
        col: in_u32(into_line_units.checked_add(1).ok_or(CoordError::Overflow)?)?,
    })
}

macro_rules! requester_encoding_support {
    ($offset:ident) => {
        // Every domain carries the same helpers; the case table exercises them
        // through one domain, so the others' copies are intentionally unused.
        #[allow(dead_code)]
        impl $offset {
            /// Checked ingress from a requester UTF-16 code-unit offset
            /// (`R03`: against the exact content snapshot). A unit offset
            /// inside a surrogate pair is [`CoordError::NotCharBoundary`];
            /// past the content's unit length is
            /// [`CoordError::InvalidBoundary`]; an extent past the `u32`
            /// coordinate width is [`CoordError::Overflow`].
            pub(super) fn from_utf16(content: &str, units: u32) -> Result<Self, CoordError> {
                if content.is_ascii() {
                    return if units as usize <= content.len() {
                        // ASCII bytes are one UTF-16 unit each, so the unit
                        // offset IS the byte offset: no narrowing happens.
                        Ok(Self(units))
                    } else {
                        Err(CoordError::InvalidBoundary)
                    };
                }
                let mut seen = 0u64;
                for (byte, ch) in content.char_indices() {
                    if seen == units as u64 {
                        return in_u32(byte as u64).map(Self);
                    }
                    let boundary = seen + ch.len_utf16() as u64;
                    if (units as u64) < boundary {
                        // Strictly inside this scalar's UTF-16 units — the
                        // second unit of a surrogate pair.
                        return Err(CoordError::NotCharBoundary);
                    }
                    seen = boundary;
                }
                if seen == units as u64 {
                    return in_u32(content.len() as u64).map(Self);
                }
                Err(CoordError::InvalidBoundary)
            }

            /// Checked egress to a UTF-16 code-unit offset, after validating
            /// this offset against the same content snapshot. A unit count
            /// past the `u32` coordinate width is
            /// [`CoordError::Overflow`].
            pub(super) fn to_utf16(self, content: &str) -> Result<u32, CoordError> {
                self.validate(content)?;
                if content.is_ascii() {
                    return Ok(self.0);
                }
                let mut units = 0u64;
                for (byte, ch) in content.char_indices() {
                    if byte == self.0 as usize {
                        return in_u32(units);
                    }
                    units += ch.len_utf16() as u64;
                }
                // validate() guarantees we only fall out at end-of-file.
                in_u32(units)
            }

            /// Checked ingress from a requester UTF-32 scalar offset. Every
            /// scalar count in extent is a scalar boundary, so only extent is
            /// refused ([`CoordError::InvalidBoundary`]; an extent past the
            /// `u32` coordinate width is [`CoordError::Overflow`]).
            pub(super) fn from_utf32(content: &str, scalars: u32) -> Result<Self, CoordError> {
                if content.is_ascii() {
                    return if scalars as usize <= content.len() {
                        // ASCII bytes are one UTF-32 scalar each, so the
                        // scalar offset IS the byte offset: no narrowing
                        // happens.
                        Ok(Self(scalars))
                    } else {
                        Err(CoordError::InvalidBoundary)
                    };
                }
                let mut seen = 0u64;
                for (byte, _) in content.char_indices() {
                    if seen == scalars as u64 {
                        return in_u32(byte as u64).map(Self);
                    }
                    seen += 1;
                }
                if seen == scalars as u64 {
                    return in_u32(content.len() as u64).map(Self);
                }
                Err(CoordError::InvalidBoundary)
            }

            /// Checked egress to a UTF-32 scalar offset, after validating
            /// this offset against the same content snapshot. A scalar count
            /// past the `u32` coordinate width is
            /// [`CoordError::Overflow`].
            pub(super) fn to_utf32(self, content: &str) -> Result<u32, CoordError> {
                self.validate(content)?;
                if content.is_ascii() {
                    return Ok(self.0);
                }
                let mut scalars = 0u64;
                for (byte, _) in content.char_indices() {
                    if byte == self.0 as usize {
                        return in_u32(scalars);
                    }
                    scalars += 1;
                }
                in_u32(scalars)
            }

            /// Convert to the TypeScript-wire line/column form ([`LineCol`]:
            /// 1-based line, 1-based UTF-16 column, the `getLineStarts`
            /// terminator set — `\n`, a lone `\r`, U+2028, U+2029, and `\r\n`
            /// as ONE terminator). Exact or refused: a byte offset between the
            /// `\r` and the `\n` of a CRLF pair lies inside a two-byte
            /// terminator and has no line/column position
            /// ([`CoordError::InvalidBoundary`]).
            pub(super) fn line_col(self, content: &str) -> Result<LineCol, CoordError> {
                self.validate(content)?;
                let mut line = 1u64;
                let mut line_start_u16 = 0u64;
                let mut u16_idx = 0u64;
                let mut chars = content.char_indices().peekable();
                while let Some((byte, ch)) = chars.next() {
                    if byte == self.0 as usize {
                        return wire_line_col(line, u16_idx - line_start_u16);
                    }
                    let mut units = ch.len_utf16() as u64;
                    let is_terminator = match ch {
                        '\n' | '\u{2028}' | '\u{2029}' => true,
                        '\r' => {
                            if chars.peek().map(|&(_, c)| c) == Some('\n') {
                                if self.0 as usize == byte + 1 {
                                    // Between the two bytes of a CRLF pair:
                                    // no line/column position exists there.
                                    return Err(CoordError::InvalidBoundary);
                                }
                                let _ = chars.next();
                                units += 1;
                            }
                            true
                        }
                        _ => false,
                    };
                    u16_idx += units;
                    if is_terminator {
                        line += 1;
                        line_start_u16 = u16_idx;
                    }
                }
                // validate() guarantees we only reach here at end-of-file.
                wire_line_col(line, u16_idx - line_start_u16)
            }

            /// Checked ingress from the TypeScript-wire line/column form
            /// ([`LineCol`]). Exact or refused: a line past the content's
            /// line count, a column more than one unit past the line's units
            /// (one unit past is the line's end boundary), or a column inside
            /// a surrogate pair ([`CoordError::NotCharBoundary`]) is a typed
            /// error — a column past the line end is never clamped to it. A
            /// byte position past the `u32` coordinate width is
            /// [`CoordError::Overflow`].
            pub(super) fn from_line_col(content: &str, pos: LineCol) -> Result<Self, CoordError> {
                if pos.line == 0 || pos.col == 0 {
                    // The form is 1-based; a 0 in either field names no
                    // position.
                    return Err(CoordError::InvalidBoundary);
                }
                let want_units = pos.col as u64 - 1;
                let mut line = 1u64;
                let mut line_start_u16 = 0u64;
                let mut u16_idx = 0u64;
                let mut chars = content.char_indices().peekable();
                while let Some((byte, ch)) = chars.next() {
                    if line == pos.line as u64 {
                        let into_line = u16_idx - line_start_u16;
                        let ch_units = ch.len_utf16() as u64;
                        if into_line == want_units {
                            return in_u32(byte as u64).map(Self);
                        }
                        if want_units < into_line + ch_units {
                            // Strictly inside this scalar's UTF-16 units.
                            return Err(CoordError::NotCharBoundary);
                        }
                    }
                    let mut units = ch.len_utf16() as u64;
                    let is_terminator = match ch {
                        '\n' | '\u{2028}' | '\u{2029}' => true,
                        '\r' => {
                            if chars.peek().map(|&(_, c)| c) == Some('\n') {
                                let _ = chars.next();
                                units += 1;
                            }
                            true
                        }
                        _ => false,
                    };
                    u16_idx += units;
                    if is_terminator {
                        line += 1;
                        line_start_u16 = u16_idx;
                    }
                }
                if line == pos.line as u64 && u16_idx - line_start_u16 == want_units {
                    // End-of-file (or a content-final line's end).
                    return in_u32(content.len() as u64).map(Self);
                }
                Err(CoordError::InvalidBoundary)
            }
        }
    };
}

requester_encoding_support!(SourceByteOffset);
requester_encoding_support!(GeneratedByteOffset);
requester_encoding_support!(EmbeddedByteOffset);
