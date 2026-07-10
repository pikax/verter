//! Batch [`CodeTransform`] operations: sorted multi-insert and
//! multi-overwrite passes that rebuild the chunk list once (O(n+m))
//! instead of paying a `Vec::insert` splice per edit.

use super::chunk::Chunk;
use super::code_transform::CodeTransform;

/// Two-channel merge cursor over already-sorted prepend slices.
///
/// Yields insertions in non-decreasing position order; at an equal position the
/// unmapped `plain` item precedes the source-mapped item — matching the order
/// of a concatenated `[plain.., mapped..]` list stably sorted by position.
/// Used by [`CodeTransform::batch_prepend_left_merged`] to fold the two prepend
/// channels into one chunk rebuild without materializing a combined Vec.
struct PrependMerge<'s, 'a> {
    /// Unmapped insertions: `(position, content)`, sorted by position.
    plain: &'s [(u32, &'a str)],
    /// Source-mapped insertions: `(position, source_start, content_offset, content)`,
    /// sorted by position.
    mapped: &'s [(u32, u32, u32, &'a str)],
    /// Cursor into `plain`.
    pi: usize,
    /// Cursor into `mapped`.
    mi: usize,
}

impl<'a> PrependMerge<'_, 'a> {
    /// Position of the next pending item, or `None` when both channels are
    /// exhausted.
    #[inline]
    fn peek_pos(&self) -> Option<u32> {
        match (self.plain.get(self.pi), self.mapped.get(self.mi)) {
            (Some(p), Some(m)) => Some(p.0.min(m.0)),
            (Some(p), None) => Some(p.0),
            (None, Some(m)) => Some(m.0),
            (None, None) => None,
        }
    }

    /// Consume the next pending item and return its chunk, advancing the
    /// cursor. The caller must confirm an item is available via
    /// [`peek_pos`](Self::peek_pos) first.
    #[inline]
    fn take_chunk(&mut self) -> Chunk<'a> {
        let take_plain = match (self.plain.get(self.pi), self.mapped.get(self.mi)) {
            // Tie → plain (unmapped) first: at an equal anchor an unmapped
            // prepend ranks ahead of a mapped one.
            (Some(p), Some(m)) => p.0 <= m.0,
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (None, None) => unreachable!("take_chunk called with both channels exhausted"),
        };
        if take_plain {
            let (_, content) = self.plain[self.pi];
            self.pi += 1;
            Chunk::inserted(content)
        } else {
            let (_, source_start, content_offset, content) = self.mapped[self.mi];
            self.mi += 1;
            Chunk::inserted_mapped_with_offset(content, source_start, content_offset)
        }
    }
}

#[allow(dead_code)] // Many API methods only exercised by tests currently
impl<'a> CodeTransform<'a> {
    /// Apply multiple prepend_left operations in a single O(n+m) pass.
    ///
    /// `items` must be sorted by position (ascending). Content strings must
    /// outlive the CodeTransform (e.g. `&'static str` for binding prefixes).
    ///
    /// This avoids O(n*m) Vec::insert cost by rebuilding the chunks Vec once.
    /// Specifically designed for batch-applying binding prefixes (`_ctx.`,
    /// `$setup.`, etc.) after all overwrites are complete.
    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    pub fn batch_prepend_left_static(&mut self, items: &[(u32, &'a str)]) -> &mut Self {
        self.record_audit_op();
        if items.is_empty() {
            return self;
        }

        // Track output delta for all insertions
        for &(_, content) in items {
            self.output_delta += content.len() as i64;
        }

        // Use scratch buffer to avoid allocation on second+ batch call
        let mut result = std::mem::take(&mut self.scratch);
        result.clear();
        let needed = self.chunks.len() + items.len() * 2;
        if result.capacity() < needed {
            result.reserve(needed - result.capacity());
        }
        let mut item_idx = 0;

        for &chunk in &self.chunks {
            match chunk {
                Chunk::Original { start: cs, end: ce } => {
                    // Emit items that fall before this chunk (in gaps between chunks)
                    while item_idx < items.len() && items[item_idx].0 < cs {
                        result.push(Chunk::inserted(items[item_idx].1));
                        item_idx += 1;
                    }

                    // Items at cs or inside (cs, ce) — split and insert
                    if item_idx < items.len() && items[item_idx].0 >= cs && items[item_idx].0 < ce {
                        let mut prev = cs;
                        while item_idx < items.len() && items[item_idx].0 < ce {
                            let pos = items[item_idx].0;
                            if pos > prev {
                                result.push(Chunk::from_source(prev, pos));
                            }
                            while item_idx < items.len() && items[item_idx].0 == pos {
                                result.push(Chunk::inserted(items[item_idx].1));
                                item_idx += 1;
                            }
                            prev = pos;
                        }
                        if prev < ce {
                            result.push(Chunk::from_source(prev, ce));
                        }
                        continue; // Don't add original chunk
                    }

                    result.push(chunk);
                }
                Chunk::Overwritten { start: cp, .. } => {
                    // For positioned non-original chunks, emit items at/before position
                    while item_idx < items.len() && items[item_idx].0 <= cp {
                        result.push(Chunk::inserted(items[item_idx].1));
                        item_idx += 1;
                    }
                    result.push(chunk);
                }
                Chunk::Inserted { .. }
                | Chunk::Moved { .. }
                | Chunk::InsertedMapped { .. }
                | Chunk::InsertedAnchored { .. } => {
                    result.push(chunk);
                }
            }
        }

        // Remaining items go at the end
        while item_idx < items.len() {
            result.push(Chunk::inserted(items[item_idx].1));
            item_idx += 1;
        }

        // Swap: old chunks become scratch for next batch call (retains capacity)
        self.scratch = std::mem::replace(&mut self.chunks, result);
        self.cursor_hint = 0;
        self
    }

    /// Apply multiple prepend_left operations with optional source map positions.
    ///
    /// Like `batch_prepend_left_static`, but each item has an optional source mapping.
    /// When `Some((source_pos, content_offset))`, creates an `InsertedMapped` chunk
    /// that emits a source map token at `source_pos`, offset within the content by
    /// `content_offset` bytes. When `None`, creates a regular `Inserted` chunk (unmapped).
    ///
    /// `items` must be sorted by insertion position (ascending).
    ///
    /// Tuple: `(insertion_pos, source_mapping, content)`
    #[allow(clippy::type_complexity)]
    pub fn batch_prepend_left_with_source_map(
        &mut self,
        items: &[(u32, Option<(u32, u32)>, &'a str)],
    ) -> &mut Self {
        self.record_audit_op();
        if items.is_empty() {
            return self;
        }

        // Track output delta for all insertions
        for &(_, _, content) in items {
            self.output_delta += content.len() as i64;
        }

        let mut result = std::mem::take(&mut self.scratch);
        result.clear();
        let needed = self.chunks.len() + items.len() * 2;
        if result.capacity() < needed {
            result.reserve(needed - result.capacity());
        }
        let mut item_idx = 0;

        for &chunk in &self.chunks {
            match chunk {
                Chunk::Original { start: cs, end: ce } => {
                    // Emit items that fall before this chunk
                    while item_idx < items.len() && items[item_idx].0 < cs {
                        let (_, source_info, content) = items[item_idx];
                        result.push(Self::make_insert_chunk(content, source_info));
                        item_idx += 1;
                    }

                    // Items at cs or inside (cs, ce) — split and insert
                    if item_idx < items.len() && items[item_idx].0 >= cs && items[item_idx].0 < ce {
                        let mut prev = cs;
                        while item_idx < items.len() && items[item_idx].0 < ce {
                            let pos = items[item_idx].0;
                            if pos > prev {
                                result.push(Chunk::from_source(prev, pos));
                            }
                            while item_idx < items.len() && items[item_idx].0 == pos {
                                let (_, source_info, content) = items[item_idx];
                                result.push(Self::make_insert_chunk(content, source_info));
                                item_idx += 1;
                            }
                            prev = pos;
                        }
                        if prev < ce {
                            result.push(Chunk::from_source(prev, ce));
                        }
                        continue;
                    }

                    result.push(chunk);
                }
                Chunk::Overwritten { start: cp, .. } => {
                    while item_idx < items.len() && items[item_idx].0 <= cp {
                        let (_, source_info, content) = items[item_idx];
                        result.push(Self::make_insert_chunk(content, source_info));
                        item_idx += 1;
                    }
                    result.push(chunk);
                }
                Chunk::Inserted { .. }
                | Chunk::Moved { .. }
                | Chunk::InsertedMapped { .. }
                | Chunk::InsertedAnchored { .. } => {
                    result.push(chunk);
                }
            }
        }

        // Remaining items go at the end
        while item_idx < items.len() {
            let (_, source_info, content) = items[item_idx];
            result.push(Self::make_insert_chunk(content, source_info));
            item_idx += 1;
        }

        self.scratch = std::mem::replace(&mut self.chunks, result);
        self.cursor_hint = 0;
        self
    }

    /// Helper to create either an InsertedMapped or Inserted chunk.
    #[inline]
    fn make_insert_chunk(content: &'a str, source_info: Option<(u32, u32)>) -> Chunk<'a> {
        match source_info {
            Some((sp, offset)) => Chunk::inserted_mapped_with_offset(content, sp, offset),
            None => Chunk::inserted(content),
        }
    }

    /// Apply the unmapped and source-mapped prepend channels in a single
    /// O(n+m) pass, merging the two already-sorted slices directly — without
    /// materializing a combined Vec.
    ///
    /// Both `plain` and `mapped` must be sorted by insertion position
    /// (ascending), each with its internal order preserved (use a stable sort).
    /// At an equal position every `plain` (unmapped) item is emitted before any
    /// `mapped` item, matching the ordering of a concatenated `[plain.., mapped..]`
    /// list stably sorted by position. `plain` items become `Inserted` chunks;
    /// `mapped` items become `InsertedMapped` chunks carrying their
    /// `(source_start, content_offset)` mapping.
    ///
    /// This is the merge counterpart of [`batch_prepend_left_static`](Self::batch_prepend_left_static)
    /// and [`batch_prepend_left_with_source_map`](Self::batch_prepend_left_with_source_map):
    /// it removes the per-apply temporary Vec that would otherwise be needed to
    /// concatenate the two channels before a single source-map-aware batch.
    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    pub fn batch_prepend_left_merged(
        &mut self,
        plain: &[(u32, &'a str)],
        mapped: &[(u32, u32, u32, &'a str)],
    ) -> &mut Self {
        self.record_audit_op();
        if plain.is_empty() && mapped.is_empty() {
            return self;
        }

        debug_assert!(
            plain.windows(2).all(|w| w[0].0 <= w[1].0),
            "batch_prepend_left_merged requires a sorted plain channel"
        );
        debug_assert!(
            mapped.windows(2).all(|w| w[0].0 <= w[1].0),
            "batch_prepend_left_merged requires a sorted mapped channel"
        );

        // Track output delta for all insertions across both channels.
        for &(_, content) in plain {
            self.output_delta += content.len() as i64;
        }
        for &(_, _, _, content) in mapped {
            self.output_delta += content.len() as i64;
        }

        // Reuse the scratch buffer to avoid allocation on second+ batch call.
        let mut result = std::mem::take(&mut self.scratch);
        result.clear();
        let needed = self.chunks.len() + (plain.len() + mapped.len()) * 2;
        if result.capacity() < needed {
            result.reserve(needed - result.capacity());
        }

        let mut merge = PrependMerge {
            plain,
            mapped,
            pi: 0,
            mi: 0,
        };

        for &chunk in &self.chunks {
            match chunk {
                Chunk::Original { start: cs, end: ce } => {
                    // Emit items that fall before this chunk (gaps between chunks).
                    while merge.peek_pos().is_some_and(|p| p < cs) {
                        result.push(merge.take_chunk());
                    }

                    // Items at cs or inside (cs, ce) — split and insert.
                    if merge.peek_pos().is_some_and(|p| p >= cs && p < ce) {
                        let mut prev = cs;
                        while let Some(pos) = merge.peek_pos() {
                            if pos >= ce {
                                break;
                            }
                            if pos > prev {
                                result.push(Chunk::from_source(prev, pos));
                            }
                            while merge.peek_pos() == Some(pos) {
                                result.push(merge.take_chunk());
                            }
                            prev = pos;
                        }
                        if prev < ce {
                            result.push(Chunk::from_source(prev, ce));
                        }
                        continue;
                    }

                    result.push(chunk);
                }
                Chunk::Overwritten { start: cp, .. } => {
                    while merge.peek_pos().is_some_and(|p| p <= cp) {
                        result.push(merge.take_chunk());
                    }
                    result.push(chunk);
                }
                Chunk::Inserted { .. }
                | Chunk::Moved { .. }
                | Chunk::InsertedMapped { .. }
                | Chunk::InsertedAnchored { .. } => {
                    result.push(chunk);
                }
            }
        }

        // Remaining items go at the end.
        while merge.peek_pos().is_some() {
            result.push(merge.take_chunk());
        }

        // Swap: old chunks become scratch for next batch call (retains capacity).
        self.scratch = std::mem::replace(&mut self.chunks, result);
        self.cursor_hint = 0;
        self
    }

    /// Apply multiple overwrite operations in a single O(n+m) pass.
    ///
    /// `overwrites` must be sorted by start position (ascending) and non-overlapping.
    /// Each entry is `(start, end, content)` — replaces source range `[start, end)`.
    ///
    /// Only affects `Original` chunks; existing `Edited` chunks pass through unchanged.
    /// This avoids O(n*m) splice cost by rebuilding the chunks Vec once.
    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    pub fn batch_overwrite(&mut self, overwrites: &[(u32, u32, &'a str)]) -> &mut Self {
        self.record_audit_op();
        if overwrites.is_empty() {
            return self;
        }

        // Precondition: inputs must be sorted by start position.
        // Overlapping ranges are tolerated — the chunk-processing loop already
        // handles them gracefully, and output_delta accounts for skipped regions.
        debug_assert!(
            overwrites.windows(2).all(|w| w[0].0 <= w[1].0),
            "batch_overwrite requires sorted ranges"
        );

        // Track output delta, accounting for overlapping ranges.
        //
        // The chunk-processing loop handles overlaps gracefully: it tracks a
        // `prev` cursor and uses `max()` to prevent it from moving backward.
        // When a later range overlaps an earlier one:
        //   - Fully contained (end <= max_end): skipped entirely, delta = 0.
        //   - Extends past max_end: content is emitted, but only the extension
        //     [max_end, end) is effectively removed from the original.
        {
            let mut max_end: u32 = 0;
            for &(start, end, content) in overwrites {
                if start >= max_end {
                    // Non-overlapping: full delta
                    self.output_delta += content.len() as i64 - (end - start) as i64;
                    max_end = end;
                } else if end > max_end {
                    // Partially overlapping but extends past max_end:
                    // content is fully emitted, only [max_end, end) is removed.
                    self.output_delta += content.len() as i64 - (end - max_end) as i64;
                    max_end = end;
                }
                // Fully contained (end <= max_end): delta = 0
            }
        }

        // Use scratch buffer to avoid allocation on second+ batch call
        let mut result = std::mem::take(&mut self.scratch);
        result.clear();
        let needed = self.chunks.len() + overwrites.len() * 2;
        if result.capacity() < needed {
            result.reserve(needed - result.capacity());
        }
        let mut ow_idx = 0;

        for &chunk in &self.chunks {
            match chunk {
                Chunk::Original { start: cs, end: ce } => {
                    // Check if any overwrites fall within [cs, ce)
                    if ow_idx < overwrites.len() && overwrites[ow_idx].0 < ce {
                        let mut prev = cs;
                        while ow_idx < overwrites.len() && overwrites[ow_idx].0 < ce {
                            let (ow_start, ow_end, ow_content) = overwrites[ow_idx];

                            // Skip overwrites that end before this chunk's start
                            if ow_end <= cs {
                                ow_idx += 1;
                                continue;
                            }

                            // Emit original content before the overwrite
                            let effective_start = ow_start.max(prev);
                            if prev < effective_start {
                                result.push(Chunk::from_source(prev, effective_start));
                            }

                            // Emit the overwritten chunk (skip empty-content deletions
                            // to reduce chunk count — the source range is still removed
                            // because prev advances past it)
                            if !ow_content.is_empty() {
                                result.push(Chunk::overwritten(ow_start, ow_end, ow_content));
                            }
                            // Use max() to prevent prev from moving backward when
                            // a later overwrite is fully contained within an earlier
                            // one's range (e.g., comment deletion inside a close-tag
                            // overwrite).
                            prev = prev.max(ow_end);
                            ow_idx += 1;
                        }

                        // Emit remaining original content after last overwrite
                        if prev < ce {
                            result.push(Chunk::from_source(prev, ce));
                        }
                    } else {
                        result.push(chunk);
                    }
                }
                Chunk::Inserted { .. }
                | Chunk::Overwritten { .. }
                | Chunk::Moved { .. }
                | Chunk::InsertedMapped { .. }
                | Chunk::InsertedAnchored { .. } => {
                    result.push(chunk);
                }
            }
        }

        // Swap: old chunks become scratch for next batch call (retains capacity)
        self.scratch = std::mem::replace(&mut self.chunks, result);
        self.cursor_hint = 0;
        self
    }
}
