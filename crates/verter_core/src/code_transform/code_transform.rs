use smallvec::SmallVec;

use super::chunk::Chunk;
use oxc_allocator::Allocator;

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
/// assert_eq!(ct.to_string(), "Hello Rust");
/// ```
pub struct CodeTransform<'a> {
    /// The original source text (never modified)
    original: &'a str,
    /// List of chunks representing the output content.
    /// Positioned chunks (Original and non-moved Edited with original_start)
    /// maintain monotonically increasing source positions.
    chunks: Vec<Chunk<'a>>,
    /// Content to prepend before everything
    intro: &'a str,
    /// Content to append after everything
    outro: &'a str,
    /// The bump allocator for string allocations
    allocator: &'a Allocator,
    /// Cursor hint: last known chunk index for a given position.
    /// Used to accelerate forward-progressing access patterns.
    cursor_hint: usize,
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
            intro: "",
            outro: "",
            allocator,
            cursor_hint: 0,
        }
    }

    /// Get the original source text
    pub fn original(&self) -> &str {
        self.original
    }

    /// Allocate a string in the bump allocator, returning a reference with the
    /// same lifetime as the CodeTransform. Useful for deferring insertions.
    pub fn alloc_str(&self, s: &str) -> &'a str {
        self.allocator.alloc_str(s)
    }

    /// Prepend content to the very start
    pub fn prepend(&mut self, content: &str) -> &mut Self {
        if !content.is_empty() {
            let new_intro = self
                .allocator
                .alloc_str(&format!("{}{}", content, self.intro));
            self.intro = new_intro;
        }
        self
    }

    /// Append content to the very end
    pub fn append(&mut self, content: &str) -> &mut Self {
        if !content.is_empty() {
            let new_outro = self
                .allocator
                .alloc_str(&format!("{}{}", self.outro, content));
            self.outro = new_outro;
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
        }
        self
    }

    /// Get the effective source position of a chunk, if it has one.
    #[inline]
    fn chunk_position(chunk: &Chunk) -> Option<u32> {
        match chunk {
            Chunk::Original { start, .. } => Some(*start),
            Chunk::Edited {
                original_start: Some(s),
                was_moved: false,
                ..
            } => Some(*s),
            _ => None,
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

    /// Split chunks at the given index if needed
    fn ensure_split_at(&mut self, index: u32) {
        let start = self.search_start_for(index);
        for i in start..self.chunks.len() {
            if let Chunk::Original { start, end } = self.chunks[i] {
                if start < index && index < end {
                    let first = Chunk::from_source(start, index);
                    let second = Chunk::from_source(index, end);
                    self.chunks[i] = first;
                    self.chunks.insert(i + 1, second);
                    if i < self.cursor_hint {
                        self.cursor_hint += 1;
                    }
                    return;
                }
                if start >= index {
                    return;
                }
            }
        }
    }

    /// Find position for prepend (inserts BEFORE existing insertions at same position).
    /// Integrates split-at-index into the same scan to avoid double traversal.
    fn find_insert_position_for_prepend(&mut self, index: u32, after: bool) -> usize {
        let start = self.search_start_for(index);

        for i in start..self.chunks.len() {
            match self.chunks[i] {
                Chunk::Original { start: cs, end: ce } => {
                    // Split needed: this chunk spans across `index`
                    if cs < index && index < ce {
                        self.chunks[i] = Chunk::from_source(cs, index);
                        self.chunks.insert(i + 1, Chunk::from_source(index, ce));
                        if i < self.cursor_hint {
                            self.cursor_hint += 1;
                        }
                        // The split produced a chunk at i+1 starting at `index`
                        self.cursor_hint = i + 1;
                        return if after { i + 2 } else { i + 1 };
                    }
                    if cs == index {
                        self.cursor_hint = i;
                        return if after { i + 1 } else { i };
                    }
                    if cs > index {
                        self.cursor_hint = i;
                        return i;
                    }
                }
                Chunk::Edited {
                    original_start: Some(s),
                    was_moved: false,
                    ..
                } => {
                    if s == index {
                        self.cursor_hint = i;
                        return if after { i + 1 } else { i };
                    }
                    if s > index {
                        self.cursor_hint = i;
                        return i;
                    }
                }
                _ => {}
            }
        }

        self.chunks.len()
    }

    /// Find position for append (inserts AFTER existing insertions at same position).
    /// Integrates split-at-index into the same scan to avoid double traversal.
    fn find_insert_position_for_append(&mut self, index: u32, after: bool) -> usize {
        let search_start = self.search_start_for(index);
        let mut result = self.chunks.len();

        for i in search_start..self.chunks.len() {
            match self.chunks[i] {
                Chunk::Original { start: cs, end: ce } => {
                    // Split needed: this chunk spans across `index`
                    if cs < index && index < ce {
                        self.chunks[i] = Chunk::from_source(cs, index);
                        self.chunks.insert(i + 1, Chunk::from_source(index, ce));
                        if i < self.cursor_hint {
                            self.cursor_hint += 1;
                        }
                        // After split, i+1 starts at `index`. For append, look past it.
                        self.cursor_hint = i + 1;
                        if !after {
                            return i + 1;
                        } else {
                            // Skip past any pure insertions after the split point
                            let mut r = i + 2;
                            for j in (i + 2)..self.chunks.len() {
                                match &self.chunks[j] {
                                    Chunk::Edited {
                                        original_start: None,
                                        ..
                                    } => {
                                        r = j + 1;
                                    }
                                    _ => break,
                                }
                            }
                            return r;
                        }
                    }
                    if cs == index {
                        self.cursor_hint = i;
                        if !after {
                            result = i;
                            break;
                        } else {
                            result = i + 1;
                            for j in (i + 1)..self.chunks.len() {
                                match &self.chunks[j] {
                                    Chunk::Edited {
                                        original_start: None,
                                        ..
                                    } => {
                                        result = j + 1;
                                    }
                                    _ => break,
                                }
                            }
                            return result;
                        }
                    }
                    if cs > index {
                        self.cursor_hint = i;
                        return i;
                    }
                }
                Chunk::Edited {
                    original_start: Some(s),
                    was_moved: false,
                    ..
                } => {
                    if s == index {
                        self.cursor_hint = i;
                        if !after {
                            result = i;
                            break;
                        } else {
                            result = i + 1;
                            for j in (i + 1)..self.chunks.len() {
                                match &self.chunks[j] {
                                    Chunk::Edited {
                                        original_start: None,
                                        ..
                                    } => {
                                        result = j + 1;
                                    }
                                    _ => break,
                                }
                            }
                            return result;
                        }
                    }
                    if s > index {
                        self.cursor_hint = i;
                        return i;
                    }
                }
                Chunk::Edited {
                    original_start: None,
                    ..
                } => {
                    result = i + 1;
                }
                Chunk::Edited {
                    was_moved: true, ..
                } => {}
            }
        }

        result
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
                Chunk::Edited {
                    original_start,
                    original_end,
                    content: _old_content,
                    was_moved,
                } => {
                    if was_moved {
                        continue;
                    }

                    let (chunk_start, chunk_end) = match (original_start, original_end) {
                        (Some(s), Some(e)) => (s, e),
                        _ => continue,
                    };

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
                        replacement_chunks.push(Chunk::overwritten(start, end, content_ref));
                        handled = true;
                    }
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
    /// assert_eq!(ct.to_string(), "CDABEF");
    /// ```
    pub fn move_slice(&mut self, start: u32, end: u32, target_index: u32) -> &mut Self {
        if start >= end {
            return self;
        }

        self.cursor_hint = 0;
        self.ensure_split_at(start);
        self.ensure_split_at(end);

        let mut indices_to_move = Vec::new();
        let mut current_pos = 0u32;

        for (i, chunk) in self.chunks.iter().enumerate() {
            match chunk {
                Chunk::Original { start: cs, end: ce } => {
                    if *cs >= start && *ce <= end {
                        indices_to_move.push(i);
                    }
                    current_pos = *ce;
                }
                Chunk::Edited {
                    original_start: Some(os),
                    original_end: Some(oe),
                    was_moved,
                    ..
                } => {
                    if *was_moved {
                        if current_pos >= start && current_pos < end {
                            indices_to_move.push(i);
                        }
                        continue;
                    }
                    if *os >= start && *oe <= end {
                        indices_to_move.push(i);
                    }
                    current_pos = *oe;
                }
                Chunk::Edited {
                    original_start: None,
                    original_end: None,
                    ..
                } => {
                    if current_pos >= start && current_pos < end {
                        indices_to_move.push(i);
                    }
                }
                Chunk::Edited {
                    original_start: Some(os),
                    original_end: None,
                    was_moved,
                    ..
                } => {
                    if *was_moved {
                        if current_pos >= start && current_pos < end {
                            indices_to_move.push(i);
                        }
                        continue;
                    }
                    if *os >= start && *os < end {
                        indices_to_move.push(i);
                    }
                }
                Chunk::Edited {
                    original_start: None,
                    original_end: Some(_),
                    ..
                } => {
                    if current_pos >= start && current_pos < end {
                        indices_to_move.push(i);
                    }
                }
            }
        }

        if indices_to_move.is_empty() {
            return self;
        }

        let mut chunks_to_move = Vec::new();
        for &i in &indices_to_move {
            let chunk = self.chunks[i];
            match chunk {
                Chunk::Original { start: cs, end: ce } => {
                    let content = self
                        .allocator
                        .alloc_str(&self.original[cs as usize..ce as usize]);
                    chunks_to_move.push(Chunk::moved(cs, ce, content));
                }
                Chunk::Edited {
                    original_start,
                    original_end,
                    content,
                    ..
                } => {
                    chunks_to_move.push(Chunk::Edited {
                        original_start,
                        original_end,
                        content,
                        was_moved: true,
                    });
                }
            }
        }

        for &i in indices_to_move.iter().rev() {
            self.chunks.remove(i);
        }

        let insert_idx = self.find_insert_position_for_append(target_index, false);
        let insert_pos = insert_idx.min(self.chunks.len());
        for (i, chunk) in chunks_to_move.into_iter().enumerate() {
            self.chunks.insert(insert_pos + i, chunk);
        }

        self
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
    /// assert_eq!(ct.to_string(), ">>CDABEF");
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
    /// assert_eq!(ct.to_string(), "CD<<ABEF");
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
    /// assert_eq!(ct.to_string(), "{CD}ABEF");
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

        self.cursor_hint = 0;
        self.ensure_split_at(start);
        self.ensure_split_at(end);

        let mut indices_to_move = Vec::new();
        let mut current_pos = 0u32;

        for (i, chunk) in self.chunks.iter().enumerate() {
            match chunk {
                Chunk::Original { start: cs, end: ce } => {
                    if *cs >= start && *ce <= end {
                        indices_to_move.push(i);
                    }
                    current_pos = *ce;
                }
                Chunk::Edited {
                    original_start: Some(os),
                    original_end: Some(oe),
                    was_moved,
                    ..
                } => {
                    if *was_moved {
                        if current_pos >= start && current_pos < end {
                            indices_to_move.push(i);
                        }
                        continue;
                    }
                    if *os >= start && *oe <= end {
                        indices_to_move.push(i);
                    }
                    current_pos = *oe;
                }
                Chunk::Edited {
                    original_start: None,
                    original_end: None,
                    ..
                } => {
                    if current_pos >= start && current_pos < end {
                        indices_to_move.push(i);
                    }
                }
                Chunk::Edited {
                    original_start: Some(os),
                    original_end: None,
                    was_moved,
                    ..
                } => {
                    if *was_moved {
                        if current_pos >= start && current_pos < end {
                            indices_to_move.push(i);
                        }
                        continue;
                    }
                    if *os >= start && *os < end {
                        indices_to_move.push(i);
                    }
                }
                Chunk::Edited {
                    original_start: None,
                    original_end: Some(_),
                    ..
                } => {
                    if current_pos >= start && current_pos < end {
                        indices_to_move.push(i);
                    }
                }
            }
        }

        if indices_to_move.is_empty() {
            return self;
        }

        let mut chunks_to_move = Vec::new();

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
                Chunk::Edited {
                    original_start,
                    original_end,
                    content,
                    ..
                } => {
                    chunks_to_move.push(Chunk::Edited {
                        original_start,
                        original_end,
                        content,
                        was_moved: true,
                    });
                }
            }
        }

        if !suffix.is_empty() {
            let suffix_ref = self.allocator.alloc_str(suffix);
            chunks_to_move.push(Chunk::inserted(suffix_ref));
        }

        for &i in indices_to_move.iter().rev() {
            self.chunks.remove(i);
        }

        let insert_idx = self.find_insert_position_for_append(target_index, false);
        let insert_pos = insert_idx.min(self.chunks.len());
        for (i, chunk) in chunks_to_move.into_iter().enumerate() {
            self.chunks.insert(insert_pos + i, chunk);
        }

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
    pub fn batch_prepend_left_static(&mut self, items: &[(u32, &'a str)]) -> &mut Self {
        if items.is_empty() {
            return self;
        }

        let mut result = Vec::with_capacity(self.chunks.len() + items.len() * 2);
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
                _ => {
                    // For positioned non-original chunks, emit items at/before position
                    if let Some(cp) = Self::chunk_position(&chunk) {
                        while item_idx < items.len() && items[item_idx].0 < cp {
                            result.push(Chunk::inserted(items[item_idx].1));
                            item_idx += 1;
                        }
                        while item_idx < items.len() && items[item_idx].0 == cp {
                            result.push(Chunk::inserted(items[item_idx].1));
                            item_idx += 1;
                        }
                    }
                    result.push(chunk);
                }
            }
        }

        // Remaining items go at the end
        while item_idx < items.len() {
            result.push(Chunk::inserted(items[item_idx].1));
            item_idx += 1;
        }

        self.chunks = result;
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
    pub fn batch_overwrite(&mut self, overwrites: &[(u32, u32, &'a str)]) -> &mut Self {
        if overwrites.is_empty() {
            return self;
        }

        // Each overwrite splits at most 1 Original into up to 3 chunks
        let mut result = Vec::with_capacity(self.chunks.len() + overwrites.len() * 2);
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
                            prev = ow_end;
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
                _ => {
                    result.push(chunk);
                }
            }
        }

        self.chunks = result;
        self.cursor_hint = 0;
        self
    }
}

impl<'a> CodeTransform<'a> {
    /// Build the final output string with pre-allocated capacity.
    pub fn to_string(&self) -> String {
        // Compute exact length to avoid reallocation during build.
        let mut total_len = self.intro.len() + self.outro.len();
        for chunk in &self.chunks {
            match chunk {
                Chunk::Original { start, end } => {
                    total_len += (*end - *start) as usize;
                }
                Chunk::Edited { content, .. } => {
                    total_len += content.len();
                }
            }
        }

        let mut out = String::with_capacity(total_len);
        out.push_str(self.intro);
        for chunk in &self.chunks {
            match chunk {
                Chunk::Original { start, end } => {
                    out.push_str(&self.original[*start as usize..*end as usize]);
                }
                Chunk::Edited { content, .. } => {
                    out.push_str(content);
                }
            }
        }
        out.push_str(self.outro);
        out
    }
}

impl<'a> std::fmt::Display for CodeTransform<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.intro)?;

        for chunk in &self.chunks {
            match chunk {
                Chunk::Original { start, end } => {
                    f.write_str(&self.original[*start as usize..*end as usize])?;
                }
                Chunk::Edited { content, .. } => {
                    f.write_str(content)?;
                }
            }
        }

        f.write_str(self.outro)
    }
}
