//! Re-expressing an existing source map through a transform's own chunk list.
//!
//! [`CodeTransform::generate_map`] answers "where did my OUTPUT bytes come from
//! in my INPUT text". [`CodeTransform::chain_source_map`] answers the composed
//! question: given a map that already describes my input text in terms of some
//! further-upstream authored source, what map describes my OUTPUT in terms of
//! that same authored source?
//!
//! Both walk the SAME chunk list and share the same
//! [`advance_generated_position`] primitive, so the two cannot disagree about
//! geometry. What differs is where the authored payload comes from: the
//! upstream map's own segments, resolved through the accepted last-applicable
//! lookup, rather than a position resolver over the transform's input.

use std::borrow::Cow;

use oxc_sourcemap::{SourceMap, Token};

use super::chunk::Chunk;
use super::code_transform::CodeTransform;
use super::source_map::advance_generated_position;

/// A transform whose shape has no chaining semantics.
///
/// Chaining is defined over a chunk list that partitions the input text into
/// retained and replaced runs. A transform that also inserts, moves, or wraps
/// content is not describable that way — the inserted bytes correspond to no
/// input position, so no upstream segment can be carried through them. Such a
/// transform is refused rather than approximated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceMapChainError {
    /// A chunk kind outside the retained/replaced partition (an insertion, a
    /// move, or a mapped insertion). Carries the offending variant's name.
    UnsupportedChunk(&'static str),
    /// The transform carries a non-empty intro; its bytes precede every input
    /// position and correspond to none.
    IntroPresent,
    /// The transform carries a non-empty outro; its bytes follow every input
    /// position and correspond to none.
    OutroPresent,
    /// The chunk list does not tile the input text left to right without gaps.
    NonContiguousChunks { expected: u32, found: u32 },
    /// An input segment names a generated position that does not exist in the
    /// transform's input text, or one that splits a surrogate pair.
    SegmentPositionOutOfBounds { line: u32, column: u32 },
}

/// The authored payload a chained segment carries: `None` for a sourceless
/// segment, otherwise the upstream segment's own `(srcLine, srcCol, srcIdx,
/// nameIdx)` unchanged.
type Payload = Option<(u32, u32, u32, Option<u32>)>;

impl<'a> CodeTransform<'a> {
    /// Re-express `input` — a map of this transform's INPUT text — as a map of
    /// this transform's OUTPUT text, carrying every authored payload unchanged.
    ///
    /// The emitted sequence is, in generated order:
    ///
    /// - every segment `input` declares at an offset inside a retained run, at
    ///   that offset's output position, in `input`'s own order (so two segments
    ///   sharing a coordinate stay in their declared order);
    /// - one segment at each non-empty replacement's generated start, carrying
    ///   the payload the replaced range's own start resolves to;
    /// - one segment where retained text RESUMES after a replacement, carrying
    ///   the payload that resume offset resolves to — without it, surviving
    ///   text would inherit the replacement's authored position;
    /// - every segment `input` declares one past the last byte, at the output's
    ///   end position.
    ///
    /// An empty replacement emits nothing and advances nothing. Every segment
    /// inside a replaced range is dropped, whatever its multiplicity.
    ///
    /// Resolution is the accepted last-applicable lookup: among the segments on
    /// the queried LINE, ordered by column and then by declared order, the last
    /// at or before the queried column. It is line-scoped, so a line with no
    /// applicable segment resolves to nothing rather than falling through to an
    /// earlier line; and it is not sourceless-transparent, so a sourceless
    /// segment is a legitimate result that stops the lookup rather than a hole
    /// to be seen through. Both properties keep a region the upstream producer
    /// deliberately left unmapped unmapped, instead of fabricating provenance
    /// for it.
    ///
    /// Tables (`sources`, `names`, `sourcesContent`, `sourceRoot`, the ignore
    /// list, `file`, `debugId`) pass through untouched — chaining moves
    /// coordinates, never identities.
    ///
    /// # Errors
    ///
    /// [`SourceMapChainError`] when this transform's shape has no chaining
    /// semantics, or when `input` names a position its input text does not have.
    pub fn chain_source_map(
        &self,
        input: &SourceMap<'_>,
    ) -> Result<SourceMap<'static>, SourceMapChainError> {
        if !self.intro().is_empty() {
            return Err(SourceMapChainError::IntroPresent);
        }
        if !self.outro().is_empty() {
            return Err(SourceMapChainError::OutroPresent);
        }

        let text = self.original();
        let text_len = text.len() as u32;
        let line_starts = line_start_offsets(text);

        let tokens: Vec<Token> = input.get_tokens().collect();

        // Each input segment's byte offset in `text`, plus a per-line index
        // ordered by (column, declared order) — exactly the order the accepted
        // lookup imposes.
        let mut offsets: Vec<u32> = Vec::with_capacity(tokens.len());
        let mut by_line: Vec<Vec<(u32, usize)>> = vec![Vec::new(); line_starts.len()];
        for (index, token) in tokens.iter().enumerate() {
            let line = token.get_dst_line();
            let column = token.get_dst_col();
            let offset = offset_of(text, &line_starts, line, column)
                .ok_or(SourceMapChainError::SegmentPositionOutOfBounds { line, column })?;
            offsets.push(offset);
            by_line[line as usize].push((column, index));
        }
        for line in &mut by_line {
            line.sort_unstable();
        }

        // The emission schedule: every input segment keyed by its offset, in
        // increasing offset order and, within one offset, in declared order.
        // Sorting by the pair rather than the offset alone makes the tie-break
        // explicit instead of resting on sort stability.
        let mut schedule: Vec<(u32, usize)> = offsets
            .iter()
            .copied()
            .enumerate()
            .map(|(index, offset)| (offset, index))
            .collect();
        schedule.sort_unstable();

        let resolve_at = |line: u32, column: u32| -> Payload {
            let candidates = &by_line[line as usize];
            let past = candidates.partition_point(|(candidate, _)| *candidate <= column);
            if past == 0 {
                return None;
            }
            payload_of(&tokens[candidates[past - 1].1])
        };

        let mut out: Vec<Token> = Vec::with_capacity(tokens.len() + self.chunks().len());
        // The position in the OUTPUT text.
        let mut gen_line = 0u32;
        let mut gen_col = 0u32;
        // The position in the INPUT text, advanced in lockstep through the same
        // bytes, so that at every chunk boundary it is exactly that boundary's
        // input position — no second offset-to-position model.
        let mut src_line = 0u32;
        let mut src_col = 0u32;
        let mut byte = 0u32;
        let mut scheduled = 0usize;
        // True when the previous chunk replaced a range, so this chunk's start
        // is where retained text resumes.
        let mut resumes_after_replacement = false;

        for chunk in self.chunks() {
            match *chunk {
                Chunk::Original { start, end } => {
                    if start != byte {
                        return Err(SourceMapChainError::NonContiguousChunks {
                            expected: byte,
                            found: start,
                        });
                    }

                    // The resume segment, suppressed when the resume offset
                    // already carries input segments: the last of those is
                    // exactly what the resume would have resolved to, so it
                    // would be a byte-identical duplicate.
                    if resumes_after_replacement
                        && !(scheduled < schedule.len() && schedule[scheduled].0 == start)
                    {
                        out.push(token_at(gen_line, gen_col, resolve_at(src_line, src_col)));
                    }

                    let mut cursor = start;
                    while scheduled < schedule.len() && schedule[scheduled].0 < end {
                        let offset = schedule[scheduled].0;
                        let span = &text[cursor as usize..offset as usize];
                        advance_generated_position(span, &mut gen_line, &mut gen_col);
                        advance_generated_position(span, &mut src_line, &mut src_col);
                        cursor = offset;

                        while scheduled < schedule.len() && schedule[scheduled].0 == offset {
                            let token = &tokens[schedule[scheduled].1];
                            out.push(token_at(gen_line, gen_col, payload_of(token)));
                            scheduled += 1;
                        }
                    }

                    let tail = &text[cursor as usize..end as usize];
                    advance_generated_position(tail, &mut gen_line, &mut gen_col);
                    advance_generated_position(tail, &mut src_line, &mut src_col);
                    byte = end;
                    resumes_after_replacement = false;
                }
                Chunk::Overwritten {
                    start,
                    end,
                    content,
                } => {
                    if start != byte {
                        return Err(SourceMapChainError::NonContiguousChunks {
                            expected: byte,
                            found: start,
                        });
                    }

                    if content.is_empty() {
                        // An empty replacement emits no segment and advances
                        // the generated position by zero.
                    } else {
                        out.push(token_at(gen_line, gen_col, resolve_at(src_line, src_col)));
                        advance_generated_position(content, &mut gen_line, &mut gen_col);
                    }

                    // Every input segment at or inside the replaced range is
                    // dropped, whatever its multiplicity.
                    while scheduled < schedule.len() && schedule[scheduled].0 < end {
                        scheduled += 1;
                    }

                    let replaced = &text[start as usize..end as usize];
                    advance_generated_position(replaced, &mut src_line, &mut src_col);
                    byte = end;
                    resumes_after_replacement = true;
                }
                Chunk::Inserted { .. } => {
                    return Err(SourceMapChainError::UnsupportedChunk("Inserted"))
                }
                Chunk::InsertedAnchored { .. } => {
                    return Err(SourceMapChainError::UnsupportedChunk("InsertedAnchored"))
                }
                Chunk::InsertedMapped { .. } => {
                    return Err(SourceMapChainError::UnsupportedChunk("InsertedMapped"))
                }
                Chunk::Moved { .. } => return Err(SourceMapChainError::UnsupportedChunk("Moved")),
                // Chain composition understands the retained/single-token-
                // replacement partition only; a segmented overwrite's
                // multi-anchor shape is a genuinely different chunk kind, so
                // it is refused exactly like the other unsupported shapes
                // above rather than approximated as a single `Overwritten`
                // token (which would silently drop every anchor but the
                // first). No existing (non-opt-in) caller can ever produce
                // this chunk kind, so this arm is unreachable for them.
                Chunk::OverwrittenSegmented { .. } => {
                    return Err(SourceMapChainError::UnsupportedChunk(
                        "OverwrittenSegmented",
                    ))
                }
            }
        }

        if byte != text_len {
            return Err(SourceMapChainError::NonContiguousChunks {
                expected: text_len,
                found: byte,
            });
        }

        // Segments one past the last byte are covered by no chunk and would
        // otherwise be silently dropped. The position is always legal — the
        // trailing empty line when the text ends with a newline, end-of-line
        // when it does not, and `(0, 0)` when the text is empty, which is also
        // the only case in which a transform with no chunks at all emits.
        while scheduled < schedule.len() {
            let (offset, index) = schedule[scheduled];
            debug_assert_eq!(offset, text_len);
            let token = &tokens[index];
            out.push(token_at(gen_line, gen_col, payload_of(token)));
            scheduled += 1;
        }

        let mut chained = SourceMap::new(
            input.get_file().map(|file| Cow::Owned(file.to_owned())),
            input
                .get_names()
                .map(|name| Cow::Owned(name.to_owned()))
                .collect(),
            input
                .get_source_root()
                .map(|root| Cow::Owned(root.to_owned())),
            input
                .get_sources()
                .map(|source| Cow::Owned(source.to_owned()))
                .collect(),
            input
                .get_source_contents()
                .map(|content| content.map(|c| Cow::Owned(c.to_owned())))
                .collect(),
            out.into_boxed_slice(),
            None,
        );
        if let Some(ignore_list) = input.get_x_google_ignore_list() {
            chained.set_x_google_ignore_list(ignore_list.to_vec());
        }
        if let Some(debug_id) = input.get_debug_id() {
            chained.set_debug_id(debug_id);
        }
        Ok(chained)
    }
}

/// A segment's authored payload, or `None` when it is sourceless.
fn payload_of(token: &Token) -> Payload {
    token.get_source_id().map(|source_id| {
        (
            token.get_src_line(),
            token.get_src_col(),
            source_id,
            token.get_name_id(),
        )
    })
}

/// A token at `(line, column)` carrying `payload`. A sourceless payload is
/// normalised to all-zero authored fields, which the wire format elides.
fn token_at(line: u32, column: u32, payload: Payload) -> Token {
    match payload {
        Some((src_line, src_col, src_idx, name_idx)) => {
            Token::new(line, column, src_line, src_col, Some(src_idx), name_idx)
        }
        None => Token::new(line, column, 0, 0, None, None),
    }
}

/// The byte offset each line of `text` starts at. Lines split on `U+000A` only
/// and retain any preceding `U+000D`, so a text ending in a newline has a
/// final, empty line and this vector has one more entry than it has newlines.
fn line_start_offsets(text: &str) -> Vec<u32> {
    let mut starts = Vec::with_capacity(8);
    starts.push(0u32);
    for (index, byte) in text.bytes().enumerate() {
        if byte == b'\n' {
            starts.push(index as u32 + 1);
        }
    }
    starts
}

/// The byte offset of a 0-based `(line, column)` position, where `column`
/// counts UTF-16 code units. `None` when the line does not exist, the column is
/// past the line's end, or the column falls strictly inside a surrogate pair —
/// in which case it addresses no character boundary and no byte offset exists.
fn offset_of(text: &str, line_starts: &[u32], line: u32, column: u32) -> Option<u32> {
    let index = line as usize;
    let start = *line_starts.get(index)? as usize;
    // A line's text excludes its terminating newline; the last line runs to the
    // end of the text.
    let end = line_starts
        .get(index + 1)
        .map_or(text.len(), |next| *next as usize - 1);

    let mut units = 0u32;
    for (byte_index, character) in text[start..end].char_indices() {
        if units == column {
            return Some((start + byte_index) as u32);
        }
        units += character.len_utf16() as u32;
        if units > column {
            // The column landed inside this character — only reachable for a
            // surrogate pair, whose two units share one character.
            return None;
        }
    }
    (units == column).then_some(end as u32)
}

#[cfg(test)]
#[path = "chain_tests.rs"]
mod chain_tests;
