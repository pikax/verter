//! Minimal non-overlapping edit geometry: the deterministic edit set between
//! authored bytes and a rendered result.
//!
//! The locked algorithm (`fixtures/formatter/laws.json`, contract FD13) uses
//! the rendered result's ordered retained provenance units as stable anchors.
//! A claimed unit is an anchor only when it is addressable in the authored
//! source, ordered after the previous anchor, and its rendered bytes are
//! exactly its authored bytes; a claim that is not addressable in this source
//! is refused with its position, and every other claim proves nothing and
//! stays gap content — including a claim whose range boundary falls inside an
//! authored CRLF pair, because the `maps.crlf` law forbids any edit boundary
//! there. In each gap between anchors the maximal equal Unicode-scalar
//! prefix is stripped, then a non-overlapping maximal equal scalar suffix, and
//! only the remaining replacement is emitted; when a replacement boundary
//! would fall inside an authored CRLF pair the replaced range extends over the
//! whole pair together with its equal stripped partner. A unit that is
//! zero-width in
//! both spaces separates nothing, so adjacent gaps coalesce through it;
//! insertions at an equal boundary follow rendered source order (walk order).
//! Anchors emit nothing, so unchanged prefixes, suffixes and internal stable
//! regions are excluded, and no whole-file replacement or global diff search
//! exists — derivation work is one linear pass plus scalar stripping.

use std::fmt;
use std::ops::Range;

use crate::coordinates::{AuthoredFormatByteRange, CoordinateError, FormattedByteRange};
use crate::render::{RenderedDoc, Segment};

/// One minimal text edit: replace `range` of the authored bytes with `text`.
///
/// An empty range is an insertion; empty text is a deletion. A derived edit is
/// never a no-op: fully equal gap content is stripped away before an edit is
/// emitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatEdit {
    /// The replaced half-open authored byte range, at scalar boundaries.
    pub range: AuthoredFormatByteRange,
    /// The rendered bytes that replace the range.
    pub text: String,
}

/// Work counters for one derivation, in the FMT0 lock's units.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EditWork {
    /// Scalar-to-scalar comparisons: retained-unit verification plus gap
    /// prefix/suffix stripping. The locked ceiling is
    /// `4*(authored_bytes + rendered_bytes) + 64`, ratified as the
    /// `edit_comparisons` bound in `catalogs/formatter-performance-v1.toml`.
    pub edit_comparisons: usize,
}

/// A derived edit set: sorted, non-overlapping edits in deterministic
/// application order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FormatEditSet {
    /// Edits sorted by authored position; at an equal boundary, insertions
    /// follow rendered source order. Consecutive edits never overlap.
    pub edits: Vec<FormatEdit>,
    /// The derivation's bounded-work counters.
    pub work: EditWork,
}

impl FormatEditSet {
    /// Apply the edits to the exact authored source and return the rendered
    /// bytes.
    ///
    /// The set must be sorted and non-overlapping, every range addressable in
    /// `source` at a scalar boundary, and free of no-op edits; a malformed set
    /// is refused with its position instead of being clamped or reordered.
    pub fn apply(&self, source: &str) -> Result<String, EditError> {
        let replaced: usize = self.edits.iter().map(|edit| edit.text.len()).sum();
        let mut applied = String::with_capacity(source.len().saturating_add(replaced));
        let mut cursor = 0;
        for (index, edit) in self.edits.iter().enumerate() {
            AuthoredFormatByteRange::for_source(source, edit.range.start(), edit.range.end())
                .map_err(|error| EditError::InvalidEdit { index, error })?;
            let start = edit.range.start().get() as usize;
            let end = edit.range.end().get() as usize;
            if edit.range.is_empty() {
                if edit.text.is_empty() {
                    return Err(EditError::NoOpEdit { index });
                }
            } else if edit.text == source[start..end] {
                return Err(EditError::NoOpEdit { index });
            }
            if start < cursor {
                return Err(EditError::EditsOutOfOrder { index });
            }
            applied.push_str(&source[cursor..start]);
            applied.push_str(&edit.text);
            cursor = edit.range.end().get() as usize;
        }
        applied.push_str(&source[cursor..]);
        Ok(applied)
    }
}

/// A derivation or application failure, with the exact reason and position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditError {
    /// The rendered segments do not tile the rendered text exactly.
    SegmentsNotContiguous { index: usize },
    /// A rendered segment's claimed range is invalid for the rendered text:
    /// out of bounds or not at a UTF-8 scalar boundary.
    InvalidRenderedRange {
        index: usize,
        error: CoordinateError,
    },
    /// A retained unit's claimed range is invalid for the authored source.
    InvalidRetainedRange {
        index: usize,
        error: CoordinateError,
    },
    /// The authored source exceeds the coordinate domain's u32 offsets.
    AuthoredTooLarge { source_len: usize },
    /// An edit starts before the previous edit's end: unsorted or overlapping.
    EditsOutOfOrder { index: usize },
    /// An edit's range is invalid for the source it is applied to.
    InvalidEdit {
        index: usize,
        error: CoordinateError,
    },
    /// An edit replaces nothing, or replaces its range with byte-identical
    /// text.
    NoOpEdit { index: usize },
}

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::SegmentsNotContiguous { index } => {
                write!(
                    f,
                    "rendered segment {index} does not tile the rendered text"
                )
            }
            Self::InvalidRenderedRange { index, error } => {
                write!(
                    f,
                    "rendered segment {index} claims an invalid range: {error}"
                )
            }
            Self::InvalidRetainedRange { index, error } => {
                write!(f, "retained unit {index} claims an invalid range: {error}")
            }
            Self::AuthoredTooLarge { source_len } => {
                write!(f, "{source_len}-byte source exceeds the u32 offset domain")
            }
            Self::EditsOutOfOrder { index } => {
                write!(f, "edit {index} starts before the previous edit's end")
            }
            Self::InvalidEdit { index, error } => {
                write!(f, "edit {index} is invalid for its source: {error}")
            }
            Self::NoOpEdit { index } => {
                write!(f, "edit {index} replaces nothing or byte-identical text")
            }
        }
    }
}

impl std::error::Error for EditError {}

/// Derive the minimal edit set from the authored bytes and one rendered
/// result: its text and the segments that produced it.
///
/// Deterministic: the same authored bytes, rendered text and segments always
/// derive the same set. See the [module](self) docs for the locked algorithm.
pub fn derive_edits(
    authored: &str,
    rendered_text: &str,
    segments: &[Segment],
) -> Result<FormatEditSet, EditError> {
    if authored.len() > u32::MAX as usize {
        return Err(EditError::AuthoredTooLarge {
            source_len: authored.len(),
        });
    }
    // The segments must tile the rendered text exactly: one contiguous cover
    // with no hole and no overhang, every endpoint addressable in that text
    // at a scalar boundary.
    let mut tiled_end = 0;
    for (index, segment) in segments.iter().enumerate() {
        let range = rendered_range(segment);
        FormattedByteRange::for_source(rendered_text, range.start(), range.end())
            .map_err(|error| EditError::InvalidRenderedRange { index, error })?;
        if range.start().get() as usize != tiled_end {
            return Err(EditError::SegmentsNotContiguous { index });
        }
        tiled_end = range.end().get() as usize;
    }
    if tiled_end != rendered_text.len() {
        return Err(EditError::SegmentsNotContiguous {
            index: segments.len(),
        });
    }
    let mut edits = Vec::new();
    let mut work = EditWork::default();
    // The open gap begins where the last accepted anchor ended, in both
    // coordinate spaces.
    let mut gap_a_start = 0;
    let mut gap_r_start = 0;
    let mut anchor_a_end = 0;
    for (index, segment) in segments.iter().enumerate() {
        let claim = match segment {
            Segment::Source(source) => *source.provenance(),
            Segment::Generated(_) => continue,
        };
        // An anchor must first be addressable in this authored source; a
        // claim that is not cannot be demoted, only refused.
        AuthoredFormatByteRange::for_source(authored, claim.range().start(), claim.range().end())
            .map_err(|error| EditError::InvalidRetainedRange { index, error })?;
        let a_start = claim.range().start().get() as usize;
        let a_end = claim.range().end().get() as usize;
        let rendered = rendered_range(segment);
        let r_start = rendered.start().get() as usize;
        let r_end = rendered.end().get() as usize;
        // A stable anchor is ordered and byte-equal on both sides. An
        // out-of-order or unequal claim is a move or a transformation, not a
        // retained unit: it stays gap content and fabricates no bijection. An
        // anchor's boundaries become edit boundaries, and the `maps.crlf` law
        // forbids an edit boundary inside an authored CRLF pair, so a claim
        // whose range boundary splits a pair proves nothing either.
        if a_start < anchor_a_end
            || splits_crlf_pair(authored, a_start)
            || splits_crlf_pair(authored, a_end)
            || !retained_equal(
                &authored[a_start..a_end],
                &rendered_text[r_start..r_end],
                &mut work,
            )
        {
            continue;
        }
        if a_start == a_end && r_start == r_end {
            // Zero-width in both spaces: separates nothing, so the gaps on
            // each side coalesce through it.
            continue;
        }
        close_gap(
            authored,
            gap_a_start..a_start,
            rendered_text,
            gap_r_start..r_start,
            &mut edits,
            &mut work,
        )?;
        gap_a_start = a_end;
        gap_r_start = r_end;
        anchor_a_end = a_end;
    }
    close_gap(
        authored,
        gap_a_start..authored.len(),
        rendered_text,
        gap_r_start..rendered_text.len(),
        &mut edits,
        &mut work,
    )?;
    Ok(FormatEditSet { edits, work })
}

/// Derive the minimal edit set from the authored bytes and one rendered
/// document, such as [`crate::render::render`] produces.
pub fn derive_edits_from_doc(
    authored: &str,
    rendered: &RenderedDoc,
) -> Result<FormatEditSet, EditError> {
    derive_edits(authored, &rendered.text, &rendered.segments)
}

fn rendered_range(segment: &Segment) -> FormattedByteRange {
    match segment {
        Segment::Source(source) => source.value().range,
        Segment::Generated(rendered) => rendered.range,
    }
}

/// Byte equality of one claimed retained unit, counting the scalar
/// comparisons it costs. A length mismatch is settled with one comparison.
fn retained_equal(authored: &str, rendered: &str, work: &mut EditWork) -> bool {
    if authored.len() != rendered.len() {
        work.edit_comparisons += 1;
        return false;
    }
    work.edit_comparisons += authored.chars().count();
    authored == rendered
}

/// Close one gap: strip the maximal equal scalar prefix, then the
/// non-overlapping maximal equal scalar suffix, and emit the remaining
/// replacement only. A gap whose content ended up equal emits nothing. A
/// replacement boundary that would fall inside an authored CRLF pair backs
/// off over the whole pair (`fixtures/formatter/laws.json`, `maps.crlf`).
fn close_gap(
    authored: &str,
    authored_gap: Range<usize>,
    rendered_text: &str,
    rendered_gap: Range<usize>,
    edits: &mut Vec<FormatEdit>,
    work: &mut EditWork,
) -> Result<(), EditError> {
    let from = &authored[authored_gap.clone()];
    let to = &rendered_text[rendered_gap];
    if from.is_empty() && to.is_empty() {
        return Ok(());
    }
    let mut prefix_from = 0;
    let mut prefix_to = 0;
    let mut from_scalars = from.char_indices();
    let mut to_scalars = to.char_indices();
    while let (Some((_, left)), Some((_, right))) = (from_scalars.next(), to_scalars.next()) {
        work.edit_comparisons += 1;
        if left != right {
            break;
        }
        prefix_from += left.len_utf8();
        prefix_to += right.len_utf8();
    }
    let from_rest = &from[prefix_from..];
    let to_rest = &to[prefix_to..];
    let mut suffix_from = 0;
    let mut suffix_to = 0;
    let mut from_scalars = from_rest.char_indices().rev();
    let mut to_scalars = to_rest.char_indices().rev();
    while let (Some((_, left)), Some((_, right))) = (from_scalars.next(), to_scalars.next()) {
        work.edit_comparisons += 1;
        if left != right {
            break;
        }
        suffix_from += left.len_utf8();
        suffix_to += right.len_utf8();
    }
    if from.len() - prefix_from - suffix_from == 0 && to.len() - prefix_to - suffix_to == 0 {
        return Ok(());
    }
    // The `maps.crlf` law: an edit boundary never falls inside an authored
    // CRLF pair. A stripping boundary that lands between the `\r` and the
    // `\n` backs off over the whole pair by un-stripping its equal rendered
    // partner, so the replacement covers the pair. Each endpoint needs at
    // most one step: the backed-off start sits before the `\r` and the
    // backed-off end after the `\n`, neither inside another pair.
    while prefix_from > 0 && splits_crlf_pair(authored, authored_gap.start + prefix_from) {
        prefix_from -= 1;
        prefix_to -= to[..prefix_to]
            .chars()
            .next_back()
            .expect("a stripped prefix scalar has an equal rendered partner")
            .len_utf8();
    }
    while suffix_from > 0 && splits_crlf_pair(authored, authored_gap.end - suffix_from) {
        suffix_from -= 1;
        suffix_to -= to[to.len() - suffix_to..]
            .chars()
            .next()
            .expect("a stripped suffix scalar has an equal rendered partner")
            .len_utf8();
    }
    let replacement_to = &to[prefix_to..to.len() - suffix_to];
    // Gap endpoints come from validated anchor claims (or the two zero
    // offsets) and stripping, including its CRLF back-off, advances on
    // scalar boundaries, so the minting below cannot fail for an
    // addressable source.
    let start = u32::try_from(authored_gap.start + prefix_from).map_err(|_| {
        EditError::AuthoredTooLarge {
            source_len: authored.len(),
        }
    })?;
    let end =
        u32::try_from(authored_gap.end - suffix_from).map_err(|_| EditError::AuthoredTooLarge {
            source_len: authored.len(),
        })?;
    edits.push(FormatEdit {
        range: AuthoredFormatByteRange::for_authored(authored, start, end)
            .expect("scalar-boundary gap geometry"),
        text: replacement_to.to_owned(),
    });
    Ok(())
}

/// Whether `offset` falls strictly inside a CRLF pair of `source`: between
/// the `\r` and the `\n`. Such a boundary may never anchor a retained unit or
/// bound an edit (`fixtures/formatter/laws.json`, `maps.crlf`).
fn splits_crlf_pair(source: &str, offset: usize) -> bool {
    offset > 0
        && offset < source.len()
        && source.as_bytes()[offset - 1] == b'\r'
        && source.as_bytes()[offset] == b'\n'
}
