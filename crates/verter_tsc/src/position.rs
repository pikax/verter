//! Human CLI positions: one-based line and column in an explicitly selected
//! code unit.
//!
//! `verter-tsc` reports `file(line,col)` with a one-based line and a one-based
//! column counted in the code unit the user selected (`--position-encoding`,
//! default `utf16`, the `tsc` convention). The engine and the source maps speak
//! UTF-16 columns; this module is the one place a requester's column unit is
//! converted, against the exact file content the position indexes. Every
//! conversion is exact or refused: a position inside a surrogate pair, past the
//! end of its line, or on a line the file does not have is a [`PositionError`],
//! and a diagnostic whose position cannot be converted is reported WITHOUT a
//! line/column rather than with a guessed one.
//!
//! Lines end at the TypeScript `getLineStarts` set: `\n`, `\r\n` (one
//! terminator), a lone `\r`, U+2028 and U+2029.

use std::fmt;

use clap::ValueEnum;

/// The code unit a column (or an `--offset`) is counted in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum)]
pub enum ColumnEncoding {
    /// UTF-8 bytes.
    #[value(name = "utf8")]
    Utf8,
    /// UTF-16 code units — the `tsc` convention and the default.
    #[default]
    #[value(name = "utf16")]
    Utf16,
    /// Unicode scalar values (UTF-32 code units).
    #[value(name = "utf32")]
    Utf32,
}

impl ColumnEncoding {
    fn unit_len(self, ch: char) -> usize {
        match self {
            Self::Utf8 => ch.len_utf8(),
            Self::Utf16 => ch.len_utf16(),
            Self::Utf32 => 1,
        }
    }
}

/// Why a position could not be converted exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PositionError {
    /// Line or column is zero (both are one-based), or the line does not
    /// exist, or the column/offset is past the end of its line/file.
    OutOfRange,
    /// The column or offset lands inside a multi-unit character in the
    /// selected encoding.
    NotCharBoundary,
}

impl fmt::Display for PositionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutOfRange => f.write_str("position is outside the file"),
            Self::NotCharBoundary => f.write_str("position is inside a multi-unit character"),
        }
    }
}

/// A line table over one file's text, built once and queried many times.
pub struct LineTable<'a> {
    text: &'a str,
    /// Byte offset of each line's first byte; `line_starts[0] == 0`.
    line_starts: std::borrow::Cow<'a, [usize]>,
}

impl<'a> LineTable<'a> {
    /// A table that scans `text` itself. Production callers keep the line
    /// starts across queries and use [`LineTable::from_parts`].
    #[cfg(test)]
    #[must_use]
    pub fn new(text: &'a str) -> Self {
        Self {
            text,
            line_starts: std::borrow::Cow::Owned(Self::line_starts(text)),
        }
    }

    /// A table over `text` whose line starts were computed once by
    /// [`LineTable::line_starts`] and are kept by the caller, so a file
    /// queried many times is scanned once.
    #[must_use]
    pub fn from_parts(text: &'a str, line_starts: &'a [usize]) -> Self {
        Self {
            text,
            line_starts: std::borrow::Cow::Borrowed(line_starts),
        }
    }

    /// Byte offset of each line's first byte in `text`; the first is `0`.
    #[must_use]
    pub fn line_starts(text: &str) -> Vec<usize> {
        let mut line_starts = vec![0usize];
        let mut chars = text.char_indices().peekable();
        while let Some((index, ch)) = chars.next() {
            let terminates = match ch {
                '\n' | '\u{2028}' | '\u{2029}' => true,
                '\r' => chars.peek().map(|&(_, next)| next) != Some('\n'),
                _ => false,
            };
            if terminates {
                line_starts.push(index + ch.len_utf8());
            }
        }
        line_starts
    }

    /// The text of 1-based `line` without its terminator.
    fn line_text(&self, line: u32) -> Result<&'a str, PositionError> {
        let index = (line as usize)
            .checked_sub(1)
            .ok_or(PositionError::OutOfRange)?;
        let start = *self
            .line_starts
            .get(index)
            .ok_or(PositionError::OutOfRange)?;
        let end = self
            .line_starts
            .get(index + 1)
            .copied()
            .unwrap_or(self.text.len());
        Ok(self.text[start..end].trim_end_matches(['\n', '\r', '\u{2028}', '\u{2029}']))
    }

    /// Re-count a 1-based column from one encoding to another on `line`,
    /// walking only within the line. Exact or refused.
    pub fn convert_column(
        &self,
        line: u32,
        column: u32,
        from: ColumnEncoding,
        to: ColumnEncoding,
    ) -> Result<u32, PositionError> {
        let target = u64::from(column.checked_sub(1).ok_or(PositionError::OutOfRange)?);
        let mut seen_from = 0u64;
        let mut seen_to = 0u64;
        for ch in self.line_text(line)?.chars() {
            if seen_from == target {
                return into_column(seen_to);
            }
            seen_from += from.unit_len(ch) as u64;
            seen_to += to.unit_len(ch) as u64;
            if target < seen_from {
                return Err(PositionError::NotCharBoundary);
            }
        }
        if seen_from == target {
            into_column(seen_to)
        } else {
            Err(PositionError::OutOfRange)
        }
    }

    /// The 1-based `(line, column)` of a 0-based `offset` counted in
    /// `encoding` units from the start of the file. Exact or refused.
    pub fn line_col_of_offset(
        &self,
        offset: u64,
        encoding: ColumnEncoding,
    ) -> Result<(u32, u32), PositionError> {
        let mut consumed = 0u64;
        for (index, &start) in self.line_starts.iter().enumerate() {
            let end = self
                .line_starts
                .get(index + 1)
                .copied()
                .unwrap_or(self.text.len());
            let line_units: u64 = self.text[start..end]
                .chars()
                .map(|ch| encoding.unit_len(ch) as u64)
                .sum();
            let is_last = index + 1 == self.line_starts.len();
            // An offset on this line's terminator (or the file end on the last
            // line) belongs to this line; the first offset of the next line
            // starts it.
            if offset < consumed + line_units || (is_last && offset == consumed + line_units) {
                let line = u32::try_from(index + 1).map_err(|_| PositionError::OutOfRange)?;
                let within = offset - consumed;
                let mut seen = 0u64;
                for ch in self.text[start..end].chars() {
                    if seen == within {
                        break;
                    }
                    seen += encoding.unit_len(ch) as u64;
                    if within < seen {
                        return Err(PositionError::NotCharBoundary);
                    }
                }
                return Ok((line, into_column(within)?));
            }
            consumed += line_units;
        }
        Err(PositionError::OutOfRange)
    }
}

fn into_column(zero_based: u64) -> Result<u32, PositionError> {
    zero_based
        .checked_add(1)
        .and_then(|column| u32::try_from(column).ok())
        .ok_or(PositionError::OutOfRange)
}

/// A position selector parsed from the command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selector {
    /// `FILE:LINE:CHAR` — one-based line and one-based column in the selected
    /// encoding.
    LineChar {
        file: String,
        line: u32,
        column: u32,
    },
    /// `FILE:OFFSET` — zero-based offset in the selected encoding from the
    /// start of the file.
    Offset { file: String, offset: u64 },
}

/// A malformed selector argument.
#[derive(Debug, PartialEq, Eq)]
pub struct SelectorParseError(pub String);

impl fmt::Display for SelectorParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for SelectorParseError {}

impl Selector {
    /// Parse `FILE:LINE:CHAR` (`--at`). The two trailing numbers are one-based;
    /// the file part may itself contain `:` (a Windows drive).
    pub fn parse_line_char(raw: &str) -> Result<Self, SelectorParseError> {
        let bad = || {
            SelectorParseError(format!(
                "'{raw}' is not FILE:LINE:CHAR (one-based line and column)"
            ))
        };
        let (rest, column) = raw.rsplit_once(':').ok_or_else(bad)?;
        let (file, line) = rest.rsplit_once(':').ok_or_else(bad)?;
        let line: u32 = line.parse().map_err(|_| bad())?;
        let column: u32 = column.parse().map_err(|_| bad())?;
        if file.is_empty() || line == 0 || column == 0 {
            return Err(SelectorParseError(format!(
                "'{raw}': line and column are one-based and the file must be named"
            )));
        }
        Ok(Self::LineChar {
            file: file.to_string(),
            line,
            column,
        })
    }

    /// Parse `FILE:OFFSET` (`--offset`). The offset is zero-based.
    pub fn parse_offset(raw: &str) -> Result<Self, SelectorParseError> {
        let bad = || SelectorParseError(format!("'{raw}' is not FILE:OFFSET (zero-based offset)"));
        let (file, offset) = raw.rsplit_once(':').ok_or_else(bad)?;
        if file.is_empty() {
            return Err(bad());
        }
        Ok(Self::Offset {
            file: file.to_string(),
            offset: offset.parse().map_err(|_| bad())?,
        })
    }

    /// The selected file as written on the command line.
    #[must_use]
    pub fn file(&self) -> &str {
        match self {
            Self::LineChar { file, .. } | Self::Offset { file, .. } => file,
        }
    }

    /// The selected one-based `(line, column)` in `encoding` units, resolving
    /// an `--offset` through the file's line table. Exact or refused.
    pub fn resolve(
        &self,
        table: &LineTable<'_>,
        encoding: ColumnEncoding,
    ) -> Result<(u32, u32), PositionError> {
        match self {
            Self::LineChar { line, column, .. } => {
                // Validate the selected position against the file now, so an
                // out-of-range selector is refused rather than matching nothing.
                table.convert_column(*line, *column, encoding, encoding)?;
                Ok((*line, *column))
            }
            Self::Offset { offset, .. } => table.line_col_of_offset(*offset, encoding),
        }
    }
}

#[cfg(test)]
#[path = "position_tests.rs"]
mod tests;
