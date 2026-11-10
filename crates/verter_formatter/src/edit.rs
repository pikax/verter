//! Minimal non-overlapping edit geometry: the deterministic edit set between
//! authored bytes and one rendered result.
//!
//! The locked algorithm (`fixtures/formatter/laws.json` `edits`, contract
//! FD13) uses the rendered result's ordered source-backed segments as stable
//! anchors. A segment's binding anchors only when its authored range is
//! addressable in the authored source, ordered after the previous anchor, not
//! bounded inside an authored CRLF pair, and its rendered bytes equal its
//! authored bytes; a binding that is not addressable is refused with its
//! position, and every other binding proves nothing and stays gap content.
//! In each gap between anchors the maximal equal Unicode-scalar prefix is
//! stripped, then a non-overlapping maximal equal scalar suffix, and only the
//! remaining replacement is emitted; a replacement boundary that would fall
//! inside an authored CRLF pair backs off over the whole pair (`maps.crlf`).
//! A unit that is zero-width in both spaces separates nothing, so the gaps on
//! each side coalesce through it; insertions at an equal boundary follow
//! rendered source order. Anchors emit nothing, so unchanged prefixes,
//! suffixes and internal stable regions are excluded, and there is no
//! whole-file replacement and no global diff search: derivation is one linear
//! pass over the segments plus scalar stripping inside each gap.

use std::fmt;
use std::ops::Range;

use crate::coordinates::{
    AuthoredFormatByteOffset, AuthoredFormatByteRange, CoordinateError, FormattedByteRange,
};
use crate::render::{RenderedDoc, Segment};

/// One text edit: replace `range` of the authored bytes with `text`.
///
/// An empty range is an insertion; empty text is a deletion. Inside a
/// [`FormatEditSet`] an edit is never a no-op.
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
    /// Scalar-to-scalar comparisons: anchor verification plus gap prefix and
    /// suffix stripping. The ratified ceiling is `4*(A_in+B_emit)+64`
    /// (`catalogs/formatter-performance-v1.toml`, `edit_comparisons`).
    pub edit_comparisons: usize,
}

/// A well-formed edit set: sorted, non-overlapping edits at scalar boundaries
/// of one authored source, never bounded inside a CRLF pair, free of no-op
/// edits, in application order.
///
/// Sets exist only through [`derive_edits`] or the checked [`Self::new`], so
/// a malformed set cannot be built; [`Self::apply`] re-checks every edit
/// against the text it is applied to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FormatEditSet {
    edits: Vec<FormatEdit>,
    work: EditWork,
}

impl FormatEditSet {
    /// Checked construction against the exact authored `source`: refuses an
    /// unsorted or overlapping edit, a range that is not addressable in
    /// `source` at scalar boundaries, a boundary inside a CRLF pair, and a
    /// no-op edit. Nothing is clamped, deduplicated or reordered.
    pub fn new(source: &str, edits: Vec<FormatEdit>) -> Result<Self, EditError> {
        validate(source, &edits)?;
        Ok(Self {
            edits,
            work: EditWork::default(),
        })
    }

    /// The edits, sorted by authored position; insertions at one boundary
    /// keep rendered source order.
    pub fn edits(&self) -> &[FormatEdit] {
        &self.edits
    }

    /// The derivation's bounded-work counters.
    pub fn work(&self) -> EditWork {
        self.work
    }

    /// The number of edits.
    pub fn len(&self) -> usize {
        self.edits.len()
    }

    /// Whether the set changes nothing: the rendered bytes equal the source.
    pub fn is_empty(&self) -> bool {
        self.edits.is_empty()
    }

    /// Apply the edits to the authored `source` and return the rendered
    /// bytes. Every edit is re-checked against `source`, and the first one
    /// that is not addressable there, splits a CRLF pair or is a no-op is
    /// refused. The set carries no source witness: a different text on which
    /// every edit still fits is not detected.
    pub fn apply(&self, source: &str) -> Result<String, EditError> {
        validate(source, &self.edits)?;
        let replaced: usize = self.edits.iter().map(|edit| edit.text.len()).sum();
        let mut applied = String::with_capacity(source.len().saturating_add(replaced));
        let mut cursor = 0;
        for edit in &self.edits {
            applied.push_str(&source[cursor..edit.range.start().get() as usize]);
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
    /// The authored source exceeds the coordinate domain's `u32` offsets.
    SourceTooLarge { source_len: usize },
    /// The rendered segments do not tile the rendered text exactly: a hole,
    /// an overlap or an overhang at segment `index` (`segments.len()` when
    /// the cover ends early).
    SegmentsNotContiguous { index: usize },
    /// A rendered segment's range is invalid for the rendered text.
    InvalidRenderedRange {
        index: usize,
        error: CoordinateError,
    },
    /// A source-backed segment's authored binding is invalid for the authored
    /// source.
    InvalidRetainedRange {
        index: usize,
        error: CoordinateError,
    },
    /// An edit starts before the previous edit's end: unsorted or overlapping.
    EditsOutOfOrder { index: usize },
    /// An edit's range is invalid for the source it is checked against.
    InvalidEdit {
        index: usize,
        error: CoordinateError,
    },
    /// An edit boundary falls between the `\r` and the `\n` of an authored
    /// CRLF pair (`fixtures/formatter/laws.json`, `maps.crlf`).
    EditSplitsCrlfPair { index: usize },
    /// An edit replaces nothing, or replaces its range with identical bytes.
    NoOpEdit { index: usize },
}

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::SourceTooLarge { source_len } => {
                write!(f, "{source_len}-byte source exceeds the u32 offset domain")
            }
            Self::SegmentsNotContiguous { index } => {
                write!(
                    f,
                    "rendered segment {index} does not tile the rendered text"
                )
            }
            Self::InvalidRenderedRange { index, error } => {
                write!(f, "rendered segment {index} has an invalid range: {error}")
            }
            Self::InvalidRetainedRange { index, error } => {
                write!(
                    f,
                    "rendered segment {index} has an invalid authored binding: {error}"
                )
            }
            Self::EditsOutOfOrder { index } => {
                write!(f, "edit {index} starts before the previous edit's end")
            }
            Self::InvalidEdit { index, error } => {
                write!(f, "edit {index} is invalid for its source: {error}")
            }
            Self::EditSplitsCrlfPair { index } => {
                write!(f, "edit {index} has a boundary inside a CRLF pair")
            }
            Self::NoOpEdit { index } => {
                write!(f, "edit {index} replaces nothing or identical bytes")
            }
        }
    }
}

impl std::error::Error for EditError {}

/// Derive the minimal edit set from the authored bytes and one rendered
/// result: its text and the segments that tile it, as [`crate::render::render`]
/// produces them.
///
/// Deterministic: the same authored bytes, rendered text and segments always
/// derive the same set. See the [module](self) docs for the locked algorithm.
pub fn derive_edits(
    authored: &str,
    rendered: &str,
    segments: &[Segment],
) -> Result<FormatEditSet, EditError> {
    if authored.len() > u32::MAX as usize {
        return Err(EditError::SourceTooLarge {
            source_len: authored.len(),
        });
    }
    // The segments must tile the rendered text exactly: one contiguous cover
    // with no hole and no overhang, every endpoint addressable in that text.
    let mut tiled_end = 0;
    for (index, segment) in segments.iter().enumerate() {
        let range = rendered_range(segment);
        FormattedByteRange::for_source(rendered, range.start(), range.end())
            .map_err(|error| EditError::InvalidRenderedRange { index, error })?;
        if range.start().get() as usize != tiled_end {
            return Err(EditError::SegmentsNotContiguous { index });
        }
        tiled_end = range.end().get() as usize;
    }
    if tiled_end != rendered.len() {
        return Err(EditError::SegmentsNotContiguous {
            index: segments.len(),
        });
    }
    // Every binding must be addressable in this authored source; one that is
    // not cannot be demoted to gap content, only refused, whatever the bytes.
    for (index, segment) in segments.iter().enumerate() {
        if let Segment::Source(source) = segment {
            let claim = source.provenance().range();
            AuthoredFormatByteRange::for_source(authored, claim.start(), claim.end())
                .map_err(|error| EditError::InvalidRetainedRange { index, error })?;
        }
    }
    let mut edits = Vec::new();
    let mut work = EditWork::default();
    // A byte-identical result changes nothing, however its segments claim
    // their bindings (`edits.second_pass`: identical output and zero edits).
    if retained_equal(authored, rendered, &mut work) {
        return Ok(FormatEditSet { edits, work });
    }
    // The open gap begins where the last accepted anchor ended, in both
    // coordinate spaces.
    let mut gap_authored = 0;
    let mut gap_rendered = 0;
    for segment in segments {
        let Segment::Source(source) = segment else {
            continue;
        };
        let claim = source.provenance().range();
        let (a_start, a_end) = (claim.start().get() as usize, claim.end().get() as usize);
        let rendered_at = source.value().range;
        let (r_start, r_end) = (
            rendered_at.start().get() as usize,
            rendered_at.end().get() as usize,
        );
        // A stable anchor is ordered and byte-equal on both sides. An
        // out-of-order or unequal binding is a move or a transformation, not
        // a retained unit: it stays gap content and fabricates no bijection.
        // Anchor boundaries become edit boundaries, and `maps.crlf` forbids an
        // edit boundary inside an authored CRLF pair, so a binding that
        // splits a pair proves nothing either.
        if a_start < gap_authored
            || splits_crlf_pair(authored, a_start)
            || splits_crlf_pair(authored, a_end)
            || !retained_equal(
                &authored[a_start..a_end],
                &rendered[r_start..r_end],
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
            gap_authored..a_start,
            rendered,
            gap_rendered..r_start,
            &mut edits,
            &mut work,
        );
        gap_authored = a_end;
        gap_rendered = r_end;
    }
    close_gap(
        authored,
        gap_authored..authored.len(),
        rendered,
        gap_rendered..rendered.len(),
        &mut edits,
        &mut work,
    );
    Ok(FormatEditSet { edits, work })
}

/// Derive the minimal edit set from the authored bytes and one rendered
/// document.
pub fn derive_edits_from_doc(
    authored: &str,
    rendered: &RenderedDoc,
) -> Result<FormatEditSet, EditError> {
    derive_edits(authored, &rendered.text, &rendered.segments)
}

/// The shared well-formedness check behind [`FormatEditSet::new`] and
/// [`FormatEditSet::apply`]: sorted, non-overlapping, addressable at scalar
/// boundaries of `source`, never bounded inside a CRLF pair, and never a
/// no-op.
fn validate(source: &str, edits: &[FormatEdit]) -> Result<(), EditError> {
    let mut cursor = 0;
    for (index, edit) in edits.iter().enumerate() {
        AuthoredFormatByteRange::for_source(source, edit.range.start(), edit.range.end())
            .map_err(|error| EditError::InvalidEdit { index, error })?;
        let (start, end) = (
            edit.range.start().get() as usize,
            edit.range.end().get() as usize,
        );
        if splits_crlf_pair(source, start) || splits_crlf_pair(source, end) {
            return Err(EditError::EditSplitsCrlfPair { index });
        }
        if start < cursor {
            return Err(EditError::EditsOutOfOrder { index });
        }
        if edit.text == source[start..end] {
            return Err(EditError::NoOpEdit { index });
        }
        cursor = end;
    }
    Ok(())
}

fn rendered_range(segment: &Segment) -> FormattedByteRange {
    match segment {
        Segment::Source(source) => source.value().range,
        Segment::Generated(generated) => generated.range,
    }
}

/// Byte equality of one claimed retained unit, counting the scalar
/// comparisons it costs. A length mismatch costs no scalar comparison.
fn retained_equal(authored: &str, rendered: &str, work: &mut EditWork) -> bool {
    if authored.len() != rendered.len() {
        return false;
    }
    // Equal lengths and equal scalars in order are equal bytes.
    authored.chars().zip(rendered.chars()).all(|(left, right)| {
        work.edit_comparisons += 1;
        left == right
    })
}

/// Strip the maximal equal scalar prefix of `from` and `to`, counting each
/// comparison; returns the stripped byte length on each side.
fn equal_prefix(from: &str, to: &str, work: &mut EditWork) -> (usize, usize) {
    let mut stripped = (0, 0);
    for (left, right) in from.chars().zip(to.chars()) {
        work.edit_comparisons += 1;
        if left != right {
            break;
        }
        stripped.0 += left.len_utf8();
        stripped.1 += right.len_utf8();
    }
    stripped
}

/// Strip the maximal equal scalar suffix of `from` and `to`, counting each
/// comparison; returns the stripped byte length on each side.
fn equal_suffix(from: &str, to: &str, work: &mut EditWork) -> (usize, usize) {
    let mut stripped = (0, 0);
    for (left, right) in from.chars().rev().zip(to.chars().rev()) {
        work.edit_comparisons += 1;
        if left != right {
            break;
        }
        stripped.0 += left.len_utf8();
        stripped.1 += right.len_utf8();
    }
    stripped
}

/// Close one gap: strip the maximal equal scalar prefix, then the
/// non-overlapping maximal equal scalar suffix, and emit only the remaining
/// replacement. A gap whose content is equal emits nothing. A replacement
/// boundary that would fall inside an authored CRLF pair backs off over the
/// whole pair (`fixtures/formatter/laws.json`, `maps.crlf`).
fn close_gap(
    authored: &str,
    authored_gap: Range<usize>,
    rendered: &str,
    rendered_gap: Range<usize>,
    edits: &mut Vec<FormatEdit>,
    work: &mut EditWork,
) {
    let from = &authored[authored_gap.clone()];
    let to = &rendered[rendered_gap];
    if from.is_empty() && to.is_empty() {
        return;
    }
    let (mut prefix_from, mut prefix_to) = equal_prefix(from, to, work);
    let (mut suffix_from, mut suffix_to) =
        equal_suffix(&from[prefix_from..], &to[prefix_to..], work);
    if prefix_from + suffix_from == from.len() && prefix_to + suffix_to == to.len() {
        return;
    }
    // A stripping boundary between the `\r` and the `\n` of an authored pair
    // backs off over the whole pair by un-stripping the `\r` (or `\n`) with
    // its equal rendered partner, so the replacement covers the pair. Each
    // endpoint needs at most one step: the backed-off start sits before the
    // `\r` and the backed-off end after the `\n`, neither inside a pair.
    if splits_crlf_pair(authored, authored_gap.start + prefix_from) {
        prefix_from -= 1;
        prefix_to -= 1;
    }
    if splits_crlf_pair(authored, authored_gap.end - suffix_from) {
        suffix_from -= 1;
        suffix_to -= 1;
    }
    let text = to[prefix_to..to.len() - suffix_to].to_owned();
    // Gap endpoints come from validated bindings or the text ends, and
    // stripping, including its CRLF back-off, moves on scalar boundaries, so
    // minting cannot fail for a source already checked against the domain.
    let offset = |at: usize| {
        AuthoredFormatByteOffset::for_source(authored, at).expect("scalar-boundary gap geometry")
    };
    let range = AuthoredFormatByteRange::for_source(
        authored,
        offset(authored_gap.start + prefix_from),
        offset(authored_gap.end - suffix_from),
    )
    .expect("ordered gap geometry");
    edits.push(FormatEdit { range, text });
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
