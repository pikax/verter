//! The checked, fail-atomic edit surface of [`CodeTransform`]: typed
//! [`CodeTransformError`] refusals, boundary-affinity insertions following
//! the `magic-string` left/right model, and the shared range-replacement
//! implementation behind both the checked (`try_*`) and unchecked
//! (`update` / `overwrite` / `remove`) range operations.

use smallvec::SmallVec;

use super::chunk::{Chunk, InsertAffinity};
use super::code_transform::CodeTransform;

/// Typed refusal from the checked (`try_*`) [`CodeTransform`] operations.
///
/// Each variant carries the offending offset/range. The checked operations
/// are fail-atomic: an `Err` mutates nothing, so a refused transform is never
/// a torn half-edit — callers abort (or surface the error) instead of
/// emitting a corrupted result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeTransformError {
    /// An offset lies past the end of the original source.
    OutOfRange { offset: u32, len: u32 },
    /// An offset falls inside a multi-byte UTF-8 character, where no chunk
    /// boundary can exist.
    MidChar { offset: u32 },
    /// A range whose start is greater than its end.
    ReversedRange { start: u32, end: u32 },
    /// A zero-length range where the operation requires a non-empty one
    /// (`try_update` / `try_overwrite` — use an insertion operation instead).
    ZeroLengthRange { offset: u32 },
    /// The operation would need a chunk boundary strictly inside content a
    /// previous edit already replaced (the `magic-string` "cannot split a
    /// chunk that has already been edited" refusal).
    ReplacedContentSplit { offset: u32 },
}

impl std::fmt::Display for CodeTransformError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OutOfRange { offset, len } => {
                write!(f, "offset {offset} is out of range (source length {len})")
            }
            Self::MidChar { offset } => {
                write!(f, "offset {offset} is not a UTF-8 character boundary")
            }
            Self::ReversedRange { start, end } => {
                write!(f, "range start {start} is greater than range end {end}")
            }
            Self::ZeroLengthRange { offset } => {
                write!(
                    f,
                    "zero-length range at offset {offset}; use an insertion operation instead"
                )
            }
            Self::ReplacedContentSplit { offset } => {
                write!(
                    f,
                    "offset {offset} falls inside content already replaced by a previous edit"
                )
            }
        }
    }
}

impl std::error::Error for CodeTransformError {}

/// The three checked insertion operations, mirroring the `magic-string`
/// insertion surface (`appendLeft` / `prependRight` / `appendRight`). The
/// affinity + stacking mode pair determines where content lands inside the
/// anchor's insertion run.
#[derive(Debug, Clone, Copy)]
enum AnchoredInsertOp {
    /// LEFT affinity, stacking in call order.
    AppendLeft,
    /// RIGHT affinity, stacking in reverse call order.
    PrependRight,
    /// RIGHT affinity, stacking in call order.
    AppendRight,
}

impl AnchoredInsertOp {
    fn affinity(self) -> InsertAffinity {
        match self {
            Self::AppendLeft => InsertAffinity::Left,
            Self::PrependRight | Self::AppendRight => InsertAffinity::Right,
        }
    }
}

#[allow(dead_code)] // Many API methods only exercised by tests currently
impl<'a> CodeTransform<'a> {
    /// The single range-replacement implementation behind `update` /
    /// `overwrite` (unchecked) and `try_update` / `try_overwrite` /
    /// `try_remove` (checked).
    ///
    /// `content_only` selects the `magic-string` `update` semantics for
    /// affinity-anchored boundary insertions; the underlying chunk splice is
    /// shared. When no anchored insertion exists the boundary passes are
    /// skipped entirely, so transforms using only the positional API keep the
    /// historical code path.
    pub(super) fn replace_range_impl(
        &mut self,
        start: u32,
        end: u32,
        content_ref: &'a str,
        content_only: bool,
    ) {
        if !self.anchored_present {
            self.splice_replace_range(start, end, content_ref);
            return;
        }

        // The nested-overwrite no-op fires inside the splice engine; detect
        // it up front so the boundary passes (and the first-chunk outro
        // extraction) are skipped with it.
        if self.is_nested_overwrite_noop(start, end) {
            return;
        }

        // Snapshot the pre-splice range topology: the end of the range's
        // first positioned chunk decides the single-chunk vs multi-chunk
        // rules below (`magic-string` splits at `start`/`end` first; those
        // boundary splits never change which chunk is first or where it
        // ends).
        let first_chunk_end = self.first_positioned_chunk_end_in(start, end);
        let multi_chunk = first_chunk_end.is_some_and(|fe| fe < end);

        // `magic-string` `update` is content-only on the FIRST chunk of the
        // range only: its end-boundary LEFT insertions survive, while every
        // other range chunk is edited non-content-only. Extract the survivors
        // before the splice clears the range interior, and re-attach them
        // right after the replacement content.
        let preserved_first_outro: SmallVec<[Chunk<'a>; 2]> = match (content_only, multi_chunk) {
            (true, true) => self.extract_anchored_at(
                first_chunk_end.expect("multi_chunk implies a first positioned chunk"),
                InsertAffinity::Left,
            ),
            _ => SmallVec::new(),
        };

        if !self.splice_replace_range(start, end, content_ref) {
            debug_assert!(
                preserved_first_outro.is_empty(),
                "the nested no-op probe must fire before the outro extraction"
            );
            return;
        }

        if !preserved_first_outro.is_empty() {
            self.reinsert_after_replacement(start, end, preserved_first_outro);
        }
        if !content_only {
            // Non-content-only edits clear the range's first-chunk RIGHT
            // insertions (`edit(content, contentOnly=false)` clears the
            // intro).
            self.clear_anchored_at(start, InsertAffinity::Right);
        }
        if !content_only || multi_chunk {
            // The range-end LEFT insertions belong to the chunk ending at
            // `end`: cleared by non-content-only edits, and by content-only
            // edits whenever that chunk is not the range's first (interior
            // chunks are always edited non-content-only).
            self.clear_anchored_at(end, InsertAffinity::Left);
        }
    }

    /// Whether [`splice_replace_range`](Self::splice_replace_range) would hit
    /// its nested-overwrite no-op for `[start, end)`: the first positioned
    /// chunk intersecting the range is an `Overwritten` chunk strictly
    /// containing it (the original source there was already replaced, so
    /// there is nothing meaningful to edit).
    fn is_nested_overwrite_noop(&self, start: u32, end: u32) -> bool {
        for chunk in &self.chunks {
            match chunk {
                Chunk::Original { end: ce, .. } => {
                    if *ce <= start {
                        continue;
                    }
                    // Past the range or intersecting it — either way the
                    // splice engine handles the replacement normally.
                    return false;
                }
                Chunk::Overwritten {
                    start: cs, end: ce, ..
                } => {
                    if *ce <= start {
                        continue;
                    }
                    if *cs >= end {
                        return false;
                    }
                    return *cs <= start && *ce >= end && (*cs < start || *ce > end);
                }
                _ => {}
            }
        }
        false
    }

    /// The end of the first positioned chunk overlapping `[start, end)`,
    /// clamped to `end` — i.e. where the range's FIRST chunk ends after the
    /// boundary splits. `None` when no positioned chunk overlaps the range.
    fn first_positioned_chunk_end_in(&self, start: u32, end: u32) -> Option<u32> {
        for chunk in &self.chunks {
            match chunk {
                Chunk::Original { start: cs, end: ce }
                | Chunk::Overwritten {
                    start: cs, end: ce, ..
                } => {
                    if *ce <= start {
                        continue;
                    }
                    if *cs >= end {
                        return None;
                    }
                    return Some((*ce).min(end));
                }
                _ => {}
            }
        }
        None
    }

    /// Remove and return (in order) every affinity-anchored insertion at
    /// `(anchor, affinity)`. The extracted chunks keep their identity — used
    /// to carry the range's first-chunk end-boundary insertions across the
    /// splice. No `output_delta` change: the caller re-inserts them.
    fn extract_anchored_at(
        &mut self,
        anchor: u32,
        affinity: InsertAffinity,
    ) -> SmallVec<[Chunk<'a>; 2]> {
        let mut extracted: SmallVec<[Chunk<'a>; 2]> = SmallVec::new();
        self.chunks.retain(|chunk| match chunk {
            Chunk::InsertedAnchored {
                anchor: a,
                affinity: af,
                ..
            } if *a == anchor && *af == affinity => {
                extracted.push(*chunk);
                false
            }
            _ => true,
        });
        if !extracted.is_empty() {
            self.cursor_hint = 0;
        }
        extracted
    }

    /// Remove every affinity-anchored insertion at `(anchor, affinity)`,
    /// subtracting the cleared content from `output_delta`.
    fn clear_anchored_at(&mut self, anchor: u32, affinity: InsertAffinity) {
        let mut removed: i64 = 0;
        self.chunks.retain(|chunk| match chunk {
            Chunk::InsertedAnchored {
                anchor: a,
                affinity: af,
                content,
            } if *a == anchor && *af == affinity => {
                removed += content.len() as i64;
                false
            }
            _ => true,
        });
        if removed != 0 {
            self.output_delta -= removed;
            self.cursor_hint = 0;
        }
    }

    /// Re-insert extracted chunks immediately after the `Overwritten` chunk
    /// produced for `[start, end)` — the position where the replaced range's
    /// preserved end-boundary insertions render.
    fn reinsert_after_replacement(
        &mut self,
        start: u32,
        end: u32,
        preserved: SmallVec<[Chunk<'a>; 2]>,
    ) {
        let insert_at = self
            .chunks
            .iter()
            .position(|chunk| {
                matches!(
                    chunk,
                    Chunk::Overwritten {
                        start: cs, end: ce, ..
                    } if *cs == start && *ce == end
                )
            })
            .map(|i| i + 1)
            .unwrap_or(self.chunks.len());
        self.chunks.splice(insert_at..insert_at, preserved);
        self.cursor_hint = 0;
    }

    // ── Checked operations: the `magic-string` edit model ──────────────────
    //
    // The `try_*` operations validate offsets and return a typed
    // [`CodeTransformError`] instead of trusting the caller, and their
    // insertions carry `magic-string` boundary affinity: at one offset,
    // LEFT-affinity content renders before RIGHT-affinity content; range
    // replacements treat RIGHT content at the range start and LEFT content
    // at a range chunk's end as attached to the range (cleared by
    // non-content-only edits) while the complementary affinities belong to
    // the neighboring chunks (always preserved). Every checked operation is
    // fail-atomic: an `Err` mutates nothing.

    /// A usable edit offset: within the original source and on a UTF-8
    /// character boundary.
    fn check_offset(&self, offset: u32) -> Result<(), CodeTransformError> {
        let len = self.original.len();
        if offset as usize > len {
            return Err(CodeTransformError::OutOfRange {
                offset,
                len: len as u32,
            });
        }
        if !self.original.is_char_boundary(offset as usize) {
            return Err(CodeTransformError::MidChar { offset });
        }
        Ok(())
    }

    /// Validate a `[start, end)` range: both offsets usable and not reversed.
    fn check_range(&self, start: u32, end: u32) -> Result<(), CodeTransformError> {
        self.check_offset(start)?;
        self.check_offset(end)?;
        if start > end {
            return Err(CodeTransformError::ReversedRange { start, end });
        }
        Ok(())
    }

    /// Register an explicit source-map segment at an authored source offset.
    /// Invalid or mid-codepoint offsets are typed refusals; this metadata never
    /// changes generated bytes.
    pub fn try_add_sourcemap_location(
        &mut self,
        offset: u32,
    ) -> Result<&mut Self, CodeTransformError> {
        self.check_offset(offset)?;
        self.sourcemap_locations.push(offset);
        Ok(self)
    }

    /// Refuse a range replacement that would need a chunk boundary strictly
    /// inside content a previous edit already replaced (the `magic-string`
    /// edited-chunk split refusal). Empty (removed) replacements split fine
    /// and are not refused.
    fn check_no_replaced_content_straddle(
        &self,
        start: u32,
        end: u32,
    ) -> Result<(), CodeTransformError> {
        for chunk in &self.chunks {
            match chunk {
                Chunk::Overwritten {
                    start: cs,
                    end: ce,
                    content,
                } => {
                    if !content.is_empty() {
                        if *cs < start && start < *ce {
                            return Err(CodeTransformError::ReplacedContentSplit { offset: start });
                        }
                        if *cs < end && end < *ce {
                            return Err(CodeTransformError::ReplacedContentSplit { offset: end });
                        }
                    }
                    if *cs >= end {
                        break;
                    }
                }
                Chunk::Original { start: cs, .. } if *cs >= end => break,
                _ => {}
            }
        }
        Ok(())
    }

    /// Ensure a positioned-chunk boundary exists at `anchor` (a VALIDATED
    /// offset), splitting the containing chunk when needed, and return the
    /// index of the first positioned chunk at-or-after `anchor` — the end
    /// bound of the anchor's insertion run.
    ///
    /// Splitting inside replaced content follows the `magic-string` rules: an
    /// empty (removed) replacement splits into two removed halves and the
    /// LEFT-affinity insertions at its end boundary transfer to the right
    /// half and immediately clear (`Chunk.split` + `edit('', false)`);
    /// content-bearing replacements refuse with `ReplacedContentSplit`.
    fn ensure_anchor_boundary(&mut self, anchor: u32) -> Result<usize, CodeTransformError> {
        let mut i = self.search_start_for(anchor);
        while i < self.chunks.len() {
            match self.chunks[i] {
                Chunk::Original { start: cs, end: ce } => {
                    if ce <= anchor {
                        i += 1;
                        continue;
                    }
                    if cs >= anchor {
                        return Ok(i);
                    }
                    self.chunks[i] = Chunk::from_source(cs, anchor);
                    self.chunks.insert(i + 1, Chunk::from_source(anchor, ce));
                    self.cursor_hint = i + 1;
                    return Ok(i + 1);
                }
                Chunk::Overwritten {
                    start: os,
                    end: oe,
                    content,
                } => {
                    if oe <= anchor {
                        i += 1;
                        continue;
                    }
                    if os >= anchor {
                        return Ok(i);
                    }
                    if !content.is_empty() {
                        return Err(CodeTransformError::ReplacedContentSplit { offset: anchor });
                    }
                    // Split a removed range into two removed halves; the
                    // right half takes the end-boundary left insertions and
                    // immediately clears them.
                    self.chunks[i] = Chunk::overwritten(os, anchor, "");
                    self.chunks
                        .insert(i + 1, Chunk::overwritten(anchor, oe, ""));
                    self.cursor_hint = i + 1;
                    let mut j = i + 2;
                    let mut removed: i64 = 0;
                    while j < self.chunks.len() {
                        match self.chunks[j] {
                            Chunk::InsertedAnchored {
                                anchor: a,
                                affinity: InsertAffinity::Left,
                                content,
                            } if a == oe => {
                                removed += content.len() as i64;
                                self.chunks.remove(j);
                            }
                            Chunk::Inserted { .. }
                            | Chunk::InsertedMapped { .. }
                            | Chunk::InsertedAnchored { .. } => {
                                j += 1;
                            }
                            _ => break,
                        }
                    }
                    if removed != 0 {
                        self.output_delta -= removed;
                    }
                    return Ok(i + 1);
                }
                _ => {
                    i += 1;
                }
            }
        }
        Ok(self.chunks.len())
    }

    /// The shared checked-insertion implementation: validate the anchor,
    /// ensure a chunk boundary, and place the content inside the anchor's
    /// insertion run according to the operation's affinity + stacking mode.
    fn try_insert_anchored(
        &mut self,
        anchor: u32,
        op: AnchoredInsertOp,
        content: &str,
    ) -> Result<(), CodeTransformError> {
        self.check_offset(anchor)?;
        let run_end = self.ensure_anchor_boundary(anchor)?;
        if content.is_empty() {
            // Nothing to insert; the boundary split (if any) still happened,
            // matching `magic-string`'s split-on-empty-insert behavior.
            return Ok(());
        }

        // The insertion run: the contiguous span of insertion chunks
        // immediately before the positioned chunk at-or-after the anchor.
        let mut run_start = run_end;
        while run_start > 0
            && matches!(
                self.chunks[run_start - 1],
                Chunk::Inserted { .. }
                    | Chunk::InsertedMapped { .. }
                    | Chunk::InsertedAnchored { .. }
            )
        {
            run_start -= 1;
        }

        let anchored_at = |chunk: &Chunk<'a>, affinity: InsertAffinity| {
            matches!(
                chunk,
                Chunk::InsertedAnchored {
                    anchor: a,
                    affinity: af,
                    ..
                } if *a == anchor && *af == affinity
            )
        };

        let insert_idx = match op {
            // After the last LEFT insertion at this anchor (call order),
            // before any RIGHT content — the left segment grows at its end.
            AnchoredInsertOp::AppendLeft => {
                let mut idx = run_start;
                for i in run_start..run_end {
                    if anchored_at(&self.chunks[i], InsertAffinity::Left) {
                        idx = i + 1;
                    }
                }
                idx
            }
            // Before the first RIGHT insertion at this anchor (reverse call
            // order).
            AnchoredInsertOp::PrependRight => {
                let mut idx = run_end;
                for i in run_start..run_end {
                    if anchored_at(&self.chunks[i], InsertAffinity::Right) {
                        idx = i;
                        break;
                    }
                }
                idx
            }
            // After the last RIGHT insertion at this anchor (call order).
            AnchoredInsertOp::AppendRight => {
                let mut idx = run_end;
                for i in run_start..run_end {
                    if anchored_at(&self.chunks[i], InsertAffinity::Right) {
                        idx = i + 1;
                    }
                }
                idx
            }
        };

        let content_ref = self.allocator.alloc_str(content);
        self.chunks.insert(
            insert_idx,
            Chunk::inserted_anchored(content_ref, anchor, op.affinity()),
        );
        if insert_idx <= self.cursor_hint {
            self.cursor_hint += 1;
        }
        self.output_delta += content.len() as i64;
        self.anchored_present = true;
        Ok(())
    }

    /// Checked left-affinity insertion at `index` (`magic-string`
    /// `appendLeft`): renders before the character at `index` and before any
    /// right-affinity content there; repeated calls stack in call order. The
    /// insertion attaches to the chunk ENDING at `index`, so it survives a
    /// later `try_update` / `try_overwrite` / `try_remove` of a range
    /// starting there.
    pub fn try_append_left(
        &mut self,
        index: u32,
        content: &str,
    ) -> Result<&mut Self, CodeTransformError> {
        self.record_audit_op();
        self.try_insert_anchored(index, AnchoredInsertOp::AppendLeft, content)?;
        Ok(self)
    }

    /// Checked right-affinity insertion at `index` (`magic-string`
    /// `prependRight`): renders before the character at `index`, after any
    /// left-affinity content there; repeated calls stack in reverse call
    /// order. The insertion attaches to the chunk STARTING at `index`, so a
    /// later non-content-only replacement of a range starting there clears
    /// it (while the content-only [`try_update`](Self::try_update) preserves
    /// it).
    pub fn try_prepend_right(
        &mut self,
        index: u32,
        content: &str,
    ) -> Result<&mut Self, CodeTransformError> {
        self.record_audit_op();
        self.try_insert_anchored(index, AnchoredInsertOp::PrependRight, content)?;
        Ok(self)
    }

    /// Checked right-affinity insertion at `index` (`magic-string`
    /// `appendRight`): like [`try_prepend_right`](Self::try_prepend_right)
    /// but repeated calls stack in call order, after the existing
    /// right-affinity content.
    pub fn try_append_right(
        &mut self,
        index: u32,
        content: &str,
    ) -> Result<&mut Self, CodeTransformError> {
        self.record_audit_op();
        self.try_insert_anchored(index, AnchoredInsertOp::AppendRight, content)?;
        Ok(self)
    }

    /// The shared checked range-replacement front end: validate, refuse
    /// splits inside replaced content, materialize the boundary splits, then
    /// run the single replacement implementation.
    fn try_replace_range(
        &mut self,
        start: u32,
        end: u32,
        content: &str,
        content_only: bool,
    ) -> Result<(), CodeTransformError> {
        self.check_range(start, end)?;
        if start == end {
            return Err(CodeTransformError::ZeroLengthRange { offset: start });
        }
        self.check_no_replaced_content_straddle(start, end)?;
        self.ensure_anchor_boundary(start)?;
        self.ensure_anchor_boundary(end)?;
        let content_ref = if content.is_empty() {
            ""
        } else {
            self.allocator.alloc_str(content)
        };
        self.replace_range_impl(start, end, content_ref, content_only);
        Ok(())
    }

    /// Checked content-only replacement (see [`update`](Self::update)):
    /// malformed offsets — out-of-range, mid-character, reversed or
    /// zero-length ranges, or a boundary inside already-replaced content —
    /// return a typed [`CodeTransformError`] without mutating anything.
    pub fn try_update(
        &mut self,
        start: u32,
        end: u32,
        content: &str,
    ) -> Result<&mut Self, CodeTransformError> {
        self.record_audit_op();
        self.try_replace_range(start, end, content, true)?;
        Ok(self)
    }

    /// Checked replacement (see [`overwrite`](Self::overwrite)): clears the
    /// boundary insertions attached to the range; malformed offsets return a
    /// typed [`CodeTransformError`] without mutating anything.
    pub fn try_overwrite(
        &mut self,
        start: u32,
        end: u32,
        content: &str,
    ) -> Result<&mut Self, CodeTransformError> {
        self.record_audit_op();
        self.try_replace_range(start, end, content, false)?;
        Ok(self)
    }

    /// Checked removal: clears the range's content and the boundary
    /// insertions attached to every chunk starting inside it (never the
    /// prior chunk's left-affinity content). A zero-length range is a no-op;
    /// malformed offsets return a typed [`CodeTransformError`] without
    /// mutating anything.
    pub fn try_remove(&mut self, start: u32, end: u32) -> Result<&mut Self, CodeTransformError> {
        self.record_audit_op();
        self.check_range(start, end)?;
        if start == end {
            return Ok(self);
        }
        self.check_no_replaced_content_straddle(start, end)?;
        self.ensure_anchor_boundary(start)?;
        self.ensure_anchor_boundary(end)?;
        self.replace_range_impl(start, end, "", false);
        Ok(self)
    }
}
