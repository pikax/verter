use smallvec::SmallVec;

use super::chunk::Chunk;
use oxc_allocator::Allocator;

/// Result of scanning chunks for a target position.
enum SplitResult {
    /// An Original chunk was split at `index`; `chunk_index` is the new
    /// chunk starting at `index` (the second half).
    SplitAt { chunk_index: usize },
    /// Found a positioned chunk starting exactly at `index`.
    ExactMatch { chunk_index: usize },
    /// First positioned chunk past `index` (no exact match found).
    PastTarget { chunk_index: usize },
    /// Reached end of chunks without finding the target.
    End,
}

/// A code transformation helper for efficient string manipulation with source map support.
///
/// This allows you to:
/// - Make surgical edits to source text (append, prepend, overwrite, remove)
/// - Track original positions for source map generation
/// - Efficiently build the final output string only when needed
/// - Use bump allocation for minimal memory overhead
///
/// # Internal Architecture
///
/// The chunks Vec is the primary storage. For forward-progressing access patterns
/// (the dominant case in template compilation), a cursor_hint accelerates lookups
/// to amortized O(1).
///
/// # Example
/// ```
/// use verter_core::code_transform::CodeTransform;
/// use oxc_allocator::Allocator;
///
/// let allocator = Allocator::default();
/// let mut ct = CodeTransform::new("Hello World", &allocator);
/// ct.overwrite(6, 11, "Rust");
/// assert_eq!(ct.build_string(), "Hello Rust");
/// ```
pub struct CodeTransform<'a> {
    /// The original source text (never modified)
    original: &'a str,
    /// List of chunks representing the output content.
    /// Positioned chunks (Original and Overwritten) maintain monotonically
    /// increasing source positions.
    chunks: Vec<Chunk<'a>>,
    /// Scratch buffer for batch operations. Swapped with `chunks` to avoid
    /// allocating a new Vec on each batch call — after the first batch op,
    /// both Vecs retain their capacity.
    scratch: Vec<Chunk<'a>>,
    /// Content to prepend before everything
    intro: &'a str,
    /// Content to append after everything
    outro: &'a str,
    /// The bump allocator for string allocations
    allocator: &'a Allocator,
    /// Cursor hint: last known chunk index for a given position.
    /// Used to accelerate forward-progressing access patterns.
    cursor_hint: usize,
    /// Running delta between output length and original length (excluding intro/outro).
    /// Tracked incrementally by each mutation to avoid a full scan in build_string().
    output_delta: i64,
    /// Whether the original source is pure ASCII.
    /// Precomputed once in `new()` to let source map generation skip `utf16_len()`
    /// calls (where byte length == UTF-16 length) for Original/Moved chunks.
    is_ascii: bool,
}

impl<'a> CodeTransform<'a> {
    /// Create a new CodeTransform from source text and an allocator
    pub fn new(source: &'a str, allocator: &'a Allocator) -> Self {
        let len = source.len() as u32;

        // Pre-allocate chunks capacity based on source size.
        // Empirically, kitchen-sink.vue (27370 bytes) produces ~2098 final chunks.
        // Using /13 avoids a Vec reallocation for typical large files.
        let estimated_chunks = if len > 0 {
            (len as usize / 13).max(64)
        } else {
            0
        };
        let mut chunks = Vec::with_capacity(estimated_chunks);
        if len > 0 {
            chunks.push(Chunk::from_source(0, len));
        }

        Self {
            original: source,
            chunks,
            scratch: Vec::with_capacity(estimated_chunks),
            intro: "",
            outro: "",
            allocator,
            cursor_hint: 0,
            output_delta: 0,
            is_ascii: source.is_ascii(),
        }
    }

    /// Get the original source text
    pub fn original(&self) -> &str {
        self.original
    }

    /// Whether the original source is pure ASCII (byte length == UTF-16 length).
    pub(super) fn is_ascii(&self) -> bool {
        self.is_ascii
    }

    /// Allocate a string in the bump allocator, returning a reference with the
    /// same lifetime as the CodeTransform. Useful for deferring insertions.
    pub fn alloc_str(&self, s: &str) -> &'a str {
        self.allocator.alloc_str(s)
    }

    /// Prepend content to the very start
    pub fn prepend(&mut self, content: &str) -> &mut Self {
        if !content.is_empty() {
            let mut buf = String::with_capacity(content.len() + self.intro.len());
            buf.push_str(content);
            buf.push_str(self.intro);
            self.intro = self.allocator.alloc_str(&buf);
        }
        self
    }

    /// Append content to the very end
    pub fn append(&mut self, content: &str) -> &mut Self {
        if !content.is_empty() {
            let mut buf = String::with_capacity(self.outro.len() + content.len());
            buf.push_str(self.outro);
            buf.push_str(content);
            self.outro = self.allocator.alloc_str(&buf);
        }
        self
    }

    /// Prepend content before a specific position (inserts before the position).
    /// Multiple prepend_left calls at the same index stack in reverse order (last call first).
    pub fn prepend_left(&mut self, index: u32, content: &str) -> &mut Self {
        if !content.is_empty() {
            let content_ref = self.allocator.alloc_str(content);
            let insert_idx = self.find_insert_position_for_prepend(index, false);
            self.chunks.insert(insert_idx, Chunk::inserted(content_ref));
            if insert_idx <= self.cursor_hint {
                self.cursor_hint += 1;
            }
            self.output_delta += content.len() as i64;
        }
        self
    }

    /// Append content before a specific position (inserts before the position).
    /// Multiple append_left calls at the same index stack in order (first call first).
    pub fn append_left(&mut self, index: u32, content: &str) -> &mut Self {
        if !content.is_empty() {
            let content_ref = self.allocator.alloc_str(content);
            let insert_idx = self.find_insert_position_for_append(index, false);
            self.chunks.insert(insert_idx, Chunk::inserted(content_ref));
            if insert_idx <= self.cursor_hint {
                self.cursor_hint += 1;
            }
            self.output_delta += content.len() as i64;
        }
        self
    }

    /// Prepend content after a specific position (inserts after the position).
    /// Multiple prepend_right calls at the same index stack in reverse order (last call first).
    pub fn prepend_right(&mut self, index: u32, content: &str) -> &mut Self {
        if !content.is_empty() {
            let content_ref = self.allocator.alloc_str(content);
            let insert_idx = self.find_insert_position_for_prepend(index, true);
            self.chunks.insert(insert_idx, Chunk::inserted(content_ref));
            if insert_idx <= self.cursor_hint {
                self.cursor_hint += 1;
            }
            self.output_delta += content.len() as i64;
        }
        self
    }

    /// Append content after a specific position (inserts after the position).
    /// Multiple append_right calls at the same index stack in order (first call first).
    pub fn append_right(&mut self, index: u32, content: &str) -> &mut Self {
        if !content.is_empty() {
            let content_ref = self.allocator.alloc_str(content);
            let insert_idx = self.find_insert_position_for_append(index, true);
            self.chunks.insert(insert_idx, Chunk::inserted(content_ref));
            if insert_idx <= self.cursor_hint {
                self.cursor_hint += 1;
            }
            self.output_delta += content.len() as i64;
        }
        self
    }

    /// Get the effective source position of a chunk, if it has one.
    /// Only Original and Overwritten chunks have positions that participate
    /// in the monotonic ordering invariant.
    #[inline]
    fn chunk_position(chunk: &Chunk) -> Option<u32> {
        match chunk {
            Chunk::Original { start, .. } | Chunk::Overwritten { start, .. } => Some(*start),
            Chunk::Inserted { .. } | Chunk::Moved { .. } => None,
        }
    }

    /// Find a good starting index for searching at the given source position.
    /// Uses the cursor hint to avoid scanning from the beginning when operations
    /// progress forward through the document (the dominant pattern in template compilation).
    ///
    /// SAFETY INVARIANT: The returned index must be <= the index of any chunk whose
    /// position is >= `index`. This means we may return an index that's slightly before
    /// the target, but NEVER after it.
    #[inline]
    fn search_start_for(&self, index: u32) -> usize {
        let hint = self.cursor_hint.min(self.chunks.len().saturating_sub(1));
        if hint == 0 || self.chunks.is_empty() {
            return 0;
        }
        // Check if the hint chunk's position is <= index (forward progress).
        // Only use the hint if we can confirm it's before or at our target.
        if let Some(pos) = Self::chunk_position(&self.chunks[hint]) {
            if pos <= index {
                return hint;
            }
        }
        // Try the chunk before the hint (common after an insert bumped the cursor)
        if hint > 0 {
            if let Some(pos) = Self::chunk_position(&self.chunks[hint - 1]) {
                if pos <= index {
                    return hint - 1;
                }
            }
        }
        // Backward jump or unknown position — must search from beginning
        // to guarantee we don't skip over affected chunks.
        0
    }

    /// Scan chunks for a target position, splitting an Original chunk if needed.
    /// Shared by both prepend and append position-finding.
    #[inline]
    fn split_and_find(&mut self, index: u32, search_start: usize) -> SplitResult {
        for i in search_start..self.chunks.len() {
            match self.chunks[i] {
                Chunk::Original { start: cs, end: ce } => {
                    if cs < index && index < ce {
                        // Split this chunk at `index`
                        self.chunks[i] = Chunk::from_source(cs, index);
                        self.chunks.insert(i + 1, Chunk::from_source(index, ce));
                        if i < self.cursor_hint {
                            self.cursor_hint += 1;
                        }
                        self.cursor_hint = i + 1;
                        return SplitResult::SplitAt { chunk_index: i + 1 };
                    }
                    if cs == index {
                        self.cursor_hint = i;
                        return SplitResult::ExactMatch { chunk_index: i };
                    }
                    if cs > index {
                        self.cursor_hint = i;
                        return SplitResult::PastTarget { chunk_index: i };
                    }
                }
                Chunk::Overwritten { start: s, .. } => {
                    // Only match at the exact start boundary. If `index` falls
                    // inside the Overwritten range (s < index < end), we skip
                    // past it — Overwritten chunks cannot be split, and inserting
                    // "at" a position inside replaced content is meaningless.
                    if s == index {
                        self.cursor_hint = i;
                        return SplitResult::ExactMatch { chunk_index: i };
                    }
                    if s > index {
                        self.cursor_hint = i;
                        return SplitResult::PastTarget { chunk_index: i };
                    }
                }
                Chunk::Inserted { .. } | Chunk::Moved { .. } => {}
            }
        }
        SplitResult::End
    }

    /// Skip past consecutive pure-insertion chunks starting at `from`,
    /// returning the index after the last one.
    #[inline]
    fn skip_pure_insertions(&self, from: usize) -> usize {
        let mut pos = from;
        for j in from..self.chunks.len() {
            if matches!(&self.chunks[j], Chunk::Inserted { .. }) {
                pos = j + 1;
            } else {
                break;
            }
        }
        pos
    }

    /// Find position for prepend (inserts BEFORE existing insertions at same position).
    fn find_insert_position_for_prepend(&mut self, index: u32, after: bool) -> usize {
        let start = self.search_start_for(index);
        match self.split_and_find(index, start) {
            SplitResult::SplitAt { chunk_index } => {
                if after {
                    chunk_index + 1
                } else {
                    chunk_index
                }
            }
            SplitResult::ExactMatch { chunk_index } => {
                if after {
                    chunk_index + 1
                } else {
                    chunk_index
                }
            }
            SplitResult::PastTarget { chunk_index } => chunk_index,
            SplitResult::End => self.chunks.len(),
        }
    }

    /// Find position for append (inserts AFTER existing insertions at same position).
    fn find_insert_position_for_append(&mut self, index: u32, after: bool) -> usize {
        let search_start = self.search_start_for(index);
        match self.split_and_find(index, search_start) {
            SplitResult::SplitAt { chunk_index } | SplitResult::ExactMatch { chunk_index } => {
                if !after {
                    chunk_index
                } else {
                    self.skip_pure_insertions(chunk_index + 1)
                }
            }
            SplitResult::PastTarget { chunk_index } => chunk_index,
            SplitResult::End => self.chunks.len(),
        }
    }

    /// Fast path for overwriting within a single Original chunk.
    /// Returns `true` if handled, `false` to fall through to the general path.
    #[inline]
    fn try_fast_overwrite(&mut self, start: u32, end: u32, content_ref: &'a str) -> bool {
        let search_start = self.search_start_for(start);
        for i in search_start..self.chunks.len() {
            match self.chunks[i] {
                Chunk::Original { start: cs, end: ce } => {
                    if ce <= start {
                        continue;
                    }
                    if cs > start {
                        // Past the target — no single-chunk match
                        return false;
                    }
                    // cs <= start && ce > start — this chunk contains `start`
                    if ce < end {
                        // Range spans past this chunk — need general path
                        return false;
                    }
                    // cs <= start && ce >= end — range fits within this one chunk
                    match (cs < start, ce > end) {
                        (true, true) => {
                            // Middle split: [cs,start) + Overwritten + [end,ce)
                            self.chunks[i] = Chunk::from_source(cs, start);
                            self.chunks
                                .insert(i + 1, Chunk::overwritten(start, end, content_ref));
                            self.chunks.insert(i + 2, Chunk::from_source(end, ce));
                            self.cursor_hint = i + 1;
                        }
                        (false, true) => {
                            // Left-aligned: Overwritten + [end,ce)
                            self.chunks[i] = Chunk::overwritten(start, end, content_ref);
                            self.chunks.insert(i + 1, Chunk::from_source(end, ce));
                            self.cursor_hint = i;
                        }
                        (true, false) => {
                            // Right-aligned: [cs,start) + Overwritten
                            self.chunks[i] = Chunk::from_source(cs, start);
                            self.chunks
                                .insert(i + 1, Chunk::overwritten(start, end, content_ref));
                            self.cursor_hint = i + 1;
                        }
                        (false, false) => {
                            // Exact match: replace in-place
                            self.chunks[i] = Chunk::overwritten(start, end, content_ref);
                            self.cursor_hint = i;
                        }
                    }
                    return true;
                }
                Chunk::Overwritten { start: s, .. } => {
                    if s > start {
                        return false; // Past target
                    }
                    // Overwritten chunk at or before start — can't use fast path
                    // (need general path to handle overlapping overwrites)
                    if s == start {
                        return false;
                    }
                }
                Chunk::Inserted { .. } | Chunk::Moved { .. } => {}
            }
        }
        false
    }

    /// Overwrite a range with new content.
    ///
    /// Uses in-place splice instead of drain+rebuild.
    pub fn overwrite(&mut self, start: u32, end: u32, content: &str) -> &mut Self {
        if start >= end {
            return self;
        }

        // Avoid bump-allocating empty strings — use a static empty slice.
        let content_ref = if content.is_empty() {
            ""
        } else {
            self.allocator.alloc_str(content)
        };

        // Track the output length change: new content minus removed original
        self.output_delta += content_ref.len() as i64 - (end - start) as i64;

        // Fast path: single Original chunk contains the entire range
        if self.try_fast_overwrite(start, end, content_ref) {
            return self;
        }

        let search_start = self.search_start_for(start);
        let mut first_affected: Option<usize> = None;
        let mut last_affected: Option<usize> = None;
        let mut replacement_chunks: SmallVec<[Chunk<'a>; 4]> = SmallVec::new();
        let mut handled = false;

        for i in search_start..self.chunks.len() {
            let chunk = self.chunks[i];
            match chunk {
                Chunk::Original {
                    start: chunk_start,
                    end: chunk_end,
                } => {
                    if chunk_end <= start {
                        continue;
                    }
                    if chunk_start >= end {
                        if !handled {
                            replacement_chunks.push(Chunk::overwritten(start, end, content_ref));
                            handled = true;
                        }
                        break;
                    }

                    if first_affected.is_none() {
                        first_affected = Some(i);
                    }
                    last_affected = Some(i);

                    if !handled {
                        if chunk_start < start {
                            replacement_chunks.push(Chunk::from_source(chunk_start, start));
                        }
                        replacement_chunks.push(Chunk::overwritten(start, end, content_ref));
                        handled = true;
                        if chunk_end > end {
                            replacement_chunks.push(Chunk::from_source(end, chunk_end));
                        }
                    } else if chunk_end > end {
                        replacement_chunks.push(Chunk::from_source(end, chunk_end));
                    }
                }
                Chunk::Overwritten {
                    start: chunk_start,
                    end: chunk_end,
                    ..
                } => {
                    if chunk_end <= start {
                        continue;
                    }
                    if chunk_start >= end {
                        if !handled {
                            replacement_chunks.push(Chunk::overwritten(start, end, content_ref));
                            handled = true;
                        }
                        break;
                    }

                    // If the new overwrite is strictly contained within this
                    // existing Overwritten chunk (smaller range), it's a no-op:
                    // the original source has already been replaced, so there's
                    // nothing meaningful to remove or overwrite. Same-range
                    // overwrites (re-overwrite) are still allowed.
                    // This prevents strip_types from destroying macro
                    // overwrites when removing generic type params.
                    if !handled
                        && chunk_start <= start
                        && chunk_end >= end
                        && (chunk_start < start || chunk_end > end)
                    {
                        // Undo the output_delta we already added for this overwrite
                        self.output_delta -= content_ref.len() as i64 - (end - start) as i64;
                        return self;
                    }

                    if first_affected.is_none() {
                        first_affected = Some(i);
                    }
                    last_affected = Some(i);

                    if !handled {
                        replacement_chunks.push(Chunk::overwritten(start, end, content_ref));
                        handled = true;
                    }
                }
                // Moved and Inserted chunks don't participate in overwrite resolution
                Chunk::Moved { .. } | Chunk::Inserted { .. } => {
                    continue;
                }
            }
        }

        if let (Some(first), Some(last)) = (first_affected, last_affected) {
            self.chunks.splice(first..=last, replacement_chunks);
            self.cursor_hint = first;
        } else if !handled {
            self.chunks
                .push(Chunk::overwritten(start, end, content_ref));
            self.cursor_hint = self.chunks.len() - 1;
        } else {
            let insert_at = first_affected.unwrap_or(self.chunks.len());
            let num = replacement_chunks.len();
            self.chunks.splice(insert_at..insert_at, replacement_chunks);
            self.cursor_hint = insert_at + num.saturating_sub(1);
        }

        self
    }

    /// Replace a range with new content (alias for overwrite)
    pub fn replace(&mut self, start: u32, end: u32, content: &str) -> &mut Self {
        self.overwrite(start, end, content)
    }

    /// Remove a range
    pub fn remove(&mut self, start: u32, end: u32) -> &mut Self {
        self.overwrite(start, end, "")
    }

    /// Move a slice from one position to another, preserving source mapping.
    ///
    /// # Example
    /// ```
    /// use verter_core::code_transform::CodeTransform;
    /// use oxc_allocator::Allocator;
    ///
    /// let allocator = Allocator::default();
    /// let mut ct = CodeTransform::new("ABCDEF", &allocator);
    /// ct.move_slice(2, 4, 0); // Move "CD" to the beginning
    /// assert_eq!(ct.build_string(), "CDABEF");
    /// ```
    pub fn move_slice(&mut self, start: u32, end: u32, target_index: u32) -> &mut Self {
        self.move_wrapped(start, end, target_index, "", "")
    }

    /// Move a slice with an unmapped prefix before it.
    ///
    /// # Example
    /// ```
    /// use verter_core::code_transform::CodeTransform;
    /// use oxc_allocator::Allocator;
    ///
    /// let allocator = Allocator::default();
    /// let mut ct = CodeTransform::new("ABCDEF", &allocator);
    /// ct.move_with_prefix(2, 4, 0, ">>"); // Move "CD" to beginning with prefix
    /// assert_eq!(ct.build_string(), ">>CDABEF");
    /// ```
    pub fn move_with_prefix(
        &mut self,
        start: u32,
        end: u32,
        target_index: u32,
        prefix: &str,
    ) -> &mut Self {
        self.move_wrapped(start, end, target_index, prefix, "")
    }

    /// Move a slice with an unmapped suffix after it.
    ///
    /// # Example
    /// ```
    /// use verter_core::code_transform::CodeTransform;
    /// use oxc_allocator::Allocator;
    ///
    /// let allocator = Allocator::default();
    /// let mut ct = CodeTransform::new("ABCDEF", &allocator);
    /// ct.move_with_suffix(2, 4, 0, "<<"); // Move "CD" to beginning with suffix
    /// assert_eq!(ct.build_string(), "CD<<ABEF");
    /// ```
    pub fn move_with_suffix(
        &mut self,
        start: u32,
        end: u32,
        target_index: u32,
        suffix: &str,
    ) -> &mut Self {
        self.move_wrapped(start, end, target_index, "", suffix)
    }

    /// Move a slice wrapped with unmapped prefix and suffix.
    ///
    /// # Example
    /// ```
    /// use verter_core::code_transform::CodeTransform;
    /// use oxc_allocator::Allocator;
    ///
    /// let allocator = Allocator::default();
    /// let mut ct = CodeTransform::new("ABCDEF", &allocator);
    /// ct.move_wrapped(2, 4, 0, "{", "}"); // Move "CD" wrapped with braces
    /// assert_eq!(ct.build_string(), "{CD}ABEF");
    /// ```
    pub fn move_wrapped(
        &mut self,
        start: u32,
        end: u32,
        target_index: u32,
        prefix: &str,
        suffix: &str,
    ) -> &mut Self {
        if start >= end {
            return self;
        }

        // Moves are net-zero for existing content, but prefix/suffix are new insertions
        self.output_delta += prefix.len() as i64 + suffix.len() as i64;

        // Single-pass: split at boundaries + collect indices in one forward scan
        // (replaces ensure_split_at(start) + ensure_split_at(end) + identification loop)
        self.cursor_hint = 0;
        let mut indices_to_move: SmallVec<[usize; 8]> = SmallVec::new();
        // Position watermark: tracks the end of the last positioned chunk we've
        // seen. Used to decide whether unpositioned chunks (Inserted/Moved) fall
        // within the [start, end) move range — they belong to the range if the
        // watermark has entered it.
        let mut current_pos = 0u32;
        let mut i = 0;
        let mut past_start = false;

        while i < self.chunks.len() {
            match self.chunks[i] {
                Chunk::Original { start: cs, end: ce } => {
                    if ce <= start {
                        // Before the range — skip
                        current_pos = ce;
                        i += 1;
                        continue;
                    }
                    if cs >= end {
                        // Past the range — done
                        break;
                    }

                    // Split at start boundary if needed
                    if cs < start && !past_start {
                        if ce > end {
                            // Triple split: [cs,start) + [start,end) + [end,ce)
                            self.chunks[i] = Chunk::from_source(cs, start);
                            self.chunks.insert(i + 1, Chunk::from_source(start, end));
                            self.chunks.insert(i + 2, Chunk::from_source(end, ce));
                            indices_to_move.push(i + 1);
                            break;
                        }
                        // Split at start: [cs,start) + [start,ce)
                        self.chunks[i] = Chunk::from_source(cs, start);
                        self.chunks.insert(i + 1, Chunk::from_source(start, ce));
                        past_start = true;
                        current_pos = start;
                        i += 1; // Advance to the [start,ce) chunk
                        continue;
                    }
                    past_start = true;

                    // Split at end boundary if needed
                    if ce > end {
                        // Split: [cs,end) + [end,ce)
                        self.chunks[i] = Chunk::from_source(cs, end);
                        self.chunks.insert(i + 1, Chunk::from_source(end, ce));
                        indices_to_move.push(i);
                        break;
                    }

                    // Fully within range
                    indices_to_move.push(i);
                    current_pos = ce;
                    i += 1;
                }
                Chunk::Overwritten {
                    start: os, end: oe, ..
                } => {
                    if oe <= start {
                        current_pos = oe;
                        i += 1;
                        continue;
                    }
                    if os >= end {
                        break;
                    }
                    past_start = true;
                    if os >= start && oe <= end {
                        indices_to_move.push(i);
                    }
                    current_pos = oe;
                    i += 1;
                }
                Chunk::Moved { .. } | Chunk::Inserted { .. } => {
                    if (past_start || current_pos >= start) && current_pos < end {
                        indices_to_move.push(i);
                    }
                    i += 1;
                }
            }
        }

        if indices_to_move.is_empty() {
            return self;
        }

        let mut chunks_to_move: SmallVec<[Chunk<'a>; 8]> = SmallVec::new();

        if !prefix.is_empty() {
            let prefix_ref = self.allocator.alloc_str(prefix);
            chunks_to_move.push(Chunk::inserted(prefix_ref));
        }

        for &i in &indices_to_move {
            let chunk = self.chunks[i];
            match chunk {
                Chunk::Original { start: cs, end: ce } => {
                    let content = self
                        .allocator
                        .alloc_str(&self.original[cs as usize..ce as usize]);
                    chunks_to_move.push(Chunk::moved(cs, ce, content));
                }
                Chunk::Overwritten {
                    start,
                    end,
                    content,
                    ..
                } => {
                    chunks_to_move.push(Chunk::moved(start, end, content));
                }
                Chunk::Moved {
                    start,
                    end,
                    content,
                    ..
                } => {
                    chunks_to_move.push(Chunk::moved(start, end, content));
                }
                Chunk::Inserted { content } => {
                    chunks_to_move.push(Chunk::inserted(content));
                }
            }
        }

        if !suffix.is_empty() {
            let suffix_ref = self.allocator.alloc_str(suffix);
            chunks_to_move.push(Chunk::inserted(suffix_ref));
        }

        // Remove moved chunks in a single O(n) pass instead of O(n*k) reverse removal
        let mut write = 0usize;
        let mut remove_idx = 0usize;
        for read in 0..self.chunks.len() {
            if remove_idx < indices_to_move.len() && indices_to_move[remove_idx] == read {
                remove_idx += 1;
            } else {
                self.chunks[write] = self.chunks[read];
                write += 1;
            }
        }
        self.chunks.truncate(write);

        // Insert moved chunks at target position
        let insert_idx = self.find_insert_position_for_append(target_index, false);
        let insert_pos = insert_idx.min(self.chunks.len());
        self.chunks.splice(insert_pos..insert_pos, chunks_to_move);

        self
    }

    /// Get a slice of the original source
    pub fn slice(&self, start: u32, end: u32) -> &str {
        &self.original[start as usize..end as usize]
    }

    /// Check if the content has been modified
    pub fn is_modified(&self) -> bool {
        !self.intro.is_empty()
            || !self.outro.is_empty()
            || self.chunks.iter().any(|c| !c.is_original())
    }

    /// Get read-only access to chunks (useful for source map generation)
    pub(super) fn chunks(&self) -> &[Chunk<'a>] {
        &self.chunks
    }

    /// Get the number of chunks (useful for diagnostics)
    #[cfg(test)]
    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    /// Get the tracked output delta (for testing capacity accuracy)
    #[cfg(test)]
    pub fn output_delta(&self) -> i64 {
        self.output_delta
    }

    /// Get the intro text
    pub(super) fn intro(&self) -> &str {
        self.intro
    }

    /// Get the outro text
    pub(super) fn outro(&self) -> &str {
        self.outro
    }

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
                Chunk::Inserted { .. } | Chunk::Moved { .. } => {
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

    /// Apply multiple overwrite operations in a single O(n+m) pass.
    ///
    /// `overwrites` must be sorted by start position (ascending) and non-overlapping.
    /// Each entry is `(start, end, content)` — replaces source range `[start, end)`.
    ///
    /// Only affects `Original` chunks; existing `Edited` chunks pass through unchanged.
    /// This avoids O(n*m) splice cost by rebuilding the chunks Vec once.
    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    pub fn batch_overwrite(&mut self, overwrites: &[(u32, u32, &'a str)]) -> &mut Self {
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
                Chunk::Inserted { .. } | Chunk::Overwritten { .. } | Chunk::Moved { .. } => {
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

impl<'a> CodeTransform<'a> {
    /// Build the final output string with pre-allocated capacity.
    #[must_use]
    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    pub fn build_string(&self) -> String {
        // Capacity from tracked delta — avoids a full scan of all chunks.
        let capacity = (self.original.len() as i64 + self.output_delta) as usize
            + self.intro.len()
            + self.outro.len();

        let mut out = String::with_capacity(capacity);
        out.push_str(self.intro);
        for chunk in &self.chunks {
            match chunk {
                Chunk::Original { start, end } => {
                    out.push_str(&self.original[*start as usize..*end as usize]);
                }
                Chunk::Inserted { content }
                | Chunk::Overwritten { content, .. }
                | Chunk::Moved { content, .. } => {
                    out.push_str(content);
                }
            }
        }
        out.push_str(self.outro);
        out
    }
}

impl<'a> CodeTransform<'a> {
    /// Write the full output to any `fmt::Write` sink.
    fn write_output_to(&self, w: &mut impl std::fmt::Write) -> std::fmt::Result {
        w.write_str(self.intro)?;
        for chunk in &self.chunks {
            match chunk {
                Chunk::Original { start, end } => {
                    w.write_str(&self.original[*start as usize..*end as usize])?;
                }
                Chunk::Inserted { content }
                | Chunk::Overwritten { content, .. }
                | Chunk::Moved { content, .. } => {
                    w.write_str(content)?;
                }
            }
        }
        w.write_str(self.outro)
    }
}

impl<'a> std::fmt::Display for CodeTransform<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.write_output_to(f)
    }
}
