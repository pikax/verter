//! The one interpretation of a piece, and the wire policy of the runtime V3
//! product: where tokens and sourceless gaps go for each kind of run.

use memchr::memchr_iter;
use verter_span::Span;

use super::piece::{Piece, Provenance};
use crate::assembly::map_input::{SourcePayload, WireSegment};
use crate::cursor::position::{utf16_len, PositionResolver};

/// Advance a generated `(line, column)` position through `content`.
///
/// Lines advance one per `U+000A`; the column counts UTF-16 code units from the
/// last `U+000A` (or, with no newline, from wherever the column already was).
/// This is the SINGLE definition of generated-position advance shared by
/// the emission writer, by `CodeTransform`'s map products and chaining, and by
/// out-of-crate assemblers that must track a write cursor in the same
/// coordinate space — so a caller composing a map alongside its own output
/// cannot end up with a second, subtly different advance model.
///
/// Successive calls compose: advancing through `a` then `b` lands where
/// advancing through `a ++ b` lands.
#[inline]
pub fn advance_generated_position(content: &str, line: &mut u32, column: &mut u32) {
    let bytes = content.as_bytes();
    let mut prev = 0usize;
    for nl_pos in memchr_iter(b'\n', bytes) {
        *line += 1;
        prev = nl_pos + 1;
    }
    if prev == 0 {
        // No newlines at all — advance column by UTF-16 length
        *column += utf16_len(content) as u32;
    } else {
        // Column is UTF-16 length from last newline to end
        *column = utf16_len(&content[prev..]) as u32;
    }
}

/// What a written piece's bytes correspond to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Correspondence {
    /// Each byte is the authored byte at the same distance from
    /// `source_start`.
    Identity { source_start: u32 },
    /// The whole run stands for `source`; an empty run is a removal.
    Rewritten { source: Span },
    /// The whole run stands for one authored point.
    Anchor { source_offset: u32 },
    /// Synthetic bytes.
    Unmapped,
    /// Bytes described by the fragment's external map at this index.
    External(u32),
}

pub(crate) fn classify(piece: &Piece<'_>) -> Correspondence {
    match piece.provenance {
        Provenance::Exact { source_start } => Correspondence::Identity { source_start },
        Provenance::Rewritten { source } => Correspondence::Rewritten { source },
        Provenance::Anchor { source_offset } => Correspondence::Anchor { source_offset },
        Provenance::Unmapped => Correspondence::Unmapped,
        Provenance::External(index) => Correspondence::External(index),
    }
}

/// Generated `(line, UTF-16 column)` of the write position, and whether a
/// mapping is active there.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct WireCursor {
    pub(crate) line: u32,
    pub(crate) column: u32,
    /// The last token on the current line maps to a source.
    pub(crate) mapped: bool,
}

impl WireCursor {
    /// Move past `text`. A V3 token applies only on its own line, so a
    /// line break ends the active mapping.
    pub(crate) fn advance(&mut self, text: &str) {
        let line = self.line;
        advance_generated_position(text, &mut self.line, &mut self.column);
        if self.line != line {
            self.mapped = false;
        }
    }

    /// A sourceless token at the write position when a mapping is active: a
    /// token applies until the next one, so synthetic bytes after mapped
    /// ones on the same line need their own.
    pub(crate) fn gap(&mut self, wire: &mut Vec<WireSegment>) {
        if self.mapped {
            self.stop(wire);
        }
    }

    /// A sourceless token at the write position, whatever is active: a
    /// boundary assembly declared.
    pub(crate) fn stop(&mut self, wire: &mut Vec<WireSegment>) {
        wire.push(WireSegment {
            generated_line: self.line,
            generated_column: self.column,
            payload: None,
        });
        self.mapped = false;
    }

    pub(crate) fn token(&mut self, wire: &mut Vec<WireSegment>, source_index: u32, at: (u32, u32)) {
        wire.push(WireSegment {
            generated_line: self.line,
            generated_column: self.column,
            payload: Some(SourcePayload {
                source_index,
                source_line: at.0,
                source_column: at.1,
                name_index: None,
            }),
        });
        self.mapped = true;
    }
}

/// The authored source a run maps into: its index in the artifact's source
/// table and its position resolver.
#[derive(Clone, Copy)]
pub(crate) struct WireSource<'r, 's> {
    pub(crate) index: u32,
    pub(crate) resolver: &'r PositionResolver<'s>,
}

impl WireSource<'_, '_> {
    fn position(&self, offset: u32) -> (u32, u32) {
        let (line, column) = self.resolver.offset_to_line_and_col(offset as usize);
        ((line - 1) as u32, (column - 1) as u32)
    }
}

/// Wire tokens for one written run `text` that `correspondence` describes,
/// advancing `cursor` past it. An identity run gets a token at its start,
/// at each interior line start and at each authored `location` it covers;
/// a rewritten or anchored run gets one token at its start; synthetic text
/// and mapped text without a source get a sourceless token when a mapping is
/// active on the current line. External runs are placed by
/// their own map and never reach this function.
pub(crate) fn wire_tokens(
    correspondence: Correspondence,
    text: &str,
    source: Option<WireSource<'_, '_>>,
    locations: &[u32],
    cursor: &mut WireCursor,
    wire: &mut Vec<WireSegment>,
) {
    if text.is_empty() {
        if correspondence == Correspondence::Unmapped {
            cursor.stop(wire);
        }
        return;
    }
    match (correspondence, source) {
        (Correspondence::Identity { source_start }, Some(source)) => {
            identity_tokens(text, source, source_start, locations, cursor, wire);
        }
        (Correspondence::Rewritten { source: span }, Some(source)) => {
            cursor.token(wire, source.index, source.position(span.start));
            cursor.advance(text);
        }
        (Correspondence::Anchor { source_offset }, Some(source)) => {
            cursor.token(wire, source.index, source.position(source_offset));
            cursor.advance(text);
        }
        (Correspondence::External(_), _) => {
            verter_debug_assert!(false, "an external run is placed by its own map");
            cursor.gap(wire);
            cursor.advance(text);
        }
        _ => {
            // A run that leaves the line at once covers no position on it.
            if !text.starts_with('\n') {
                cursor.gap(wire);
            }
            cursor.advance(text);
        }
    }
}

/// One pass over `text`, merging line starts with the authored locations
/// inside the run. Both streams are ordered, so the work is linear in the
/// text plus the tokens written.
fn identity_tokens(
    text: &str,
    source: WireSource<'_, '_>,
    source_start: u32,
    locations: &[u32],
    cursor: &mut WireCursor,
    wire: &mut Vec<WireSegment>,
) {
    let len = text.len();
    let source_end = source_start.saturating_add(len as u32);
    let mut source_position = source.position(source_start);
    cursor.token(wire, source.index, source_position);

    let first = locations.partition_point(|location| *location <= source_start);
    let mut explicit = locations[first..]
        .iter()
        .take_while(|location| **location < source_end)
        .map(|location| (location - source_start) as usize)
        .filter(|relative| text.is_char_boundary(*relative))
        .peekable();
    let mut newlines = memchr_iter(b'\n', text.as_bytes())
        .map(|position| position + 1)
        .filter(|position| *position < len)
        .peekable();
    let mut previous = 0usize;
    loop {
        let boundary = match (explicit.peek().copied(), newlines.peek().copied()) {
            (Some(explicit), Some(newline)) => explicit.min(newline),
            (Some(explicit), None) => explicit,
            (None, Some(newline)) => newline,
            (None, None) => break,
        };
        if explicit.peek() == Some(&boundary) {
            explicit.next();
        }
        if newlines.peek() == Some(&boundary) {
            newlines.next();
        }
        let traversed = &text[previous..boundary];
        cursor.advance(traversed);
        advance_generated_position(traversed, &mut source_position.0, &mut source_position.1);
        cursor.token(wire, source.index, source_position);
        previous = boundary;
    }
    cursor.advance(&text[previous..]);
}
