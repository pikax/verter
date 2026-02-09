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
    /// List of chunks representing the output content
    chunks: Vec<Chunk<'a>>,
    /// Content to prepend before everything
    intro: &'a str,
    /// Content to append after everything
    outro: &'a str,
    /// The bump allocator for string allocations
    allocator: &'a Allocator,
}

impl<'a> CodeTransform<'a> {
    /// Create a new CodeTransform from source text and an allocator
    pub fn new(source: &'a str, allocator: &'a Allocator) -> Self {
        let len = source.len() as u32;

        // Start with a single chunk referencing the entire original source
        let chunks = if len > 0 {
            vec![Chunk::from_source(0, len)]
        } else {
            vec![]
        };

        Self {
            original: source,
            chunks,
            intro: "",
            outro: "",
            allocator,
        }
    }

    /// Get the original source text
    pub fn original(&self) -> &str {
        self.original
    }

    /// Prepend content to the very start
    pub fn prepend(&mut self, content: &str) -> &mut Self {
        if !content.is_empty() {
            // Allocate new string that combines new content with existing intro
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
            // Allocate new string that combines existing outro with new content
            let new_outro = self
                .allocator
                .alloc_str(&format!("{}{}", self.outro, content));
            self.outro = new_outro;
        }
        self
    }

    /// Prepend content before a specific position (inserts before the position).
    /// Multiple prepend_left calls at the same index stack in reverse order (last call first).
    /// Example: prepend_left(5, "A"); prepend_left(5, "B"); => "BA" appears before position 5
    pub fn prepend_left(&mut self, index: u32, content: &str) -> &mut Self {
        if !content.is_empty() {
            let content_ref = self.allocator.alloc_str(content);
            let insert_idx = self.find_insert_position_for_prepend(index, false);
            self.chunks.insert(insert_idx, Chunk::inserted(content_ref));
        }
        self
    }

    /// Append content before a specific position (inserts before the position).
    /// Multiple append_left calls at the same index stack in order (first call first).
    /// Example: append_left(5, "A"); append_left(5, "B"); => "AB" appears before position 5
    pub fn append_left(&mut self, index: u32, content: &str) -> &mut Self {
        if !content.is_empty() {
            let content_ref = self.allocator.alloc_str(content);
            let insert_idx = self.find_insert_position_for_append(index, false);
            self.chunks.insert(insert_idx, Chunk::inserted(content_ref));
        }
        self
    }

    /// Prepend content after a specific position (inserts after the position).
    /// Multiple prepend_right calls at the same index stack in reverse order (last call first).
    /// Example: prepend_right(5, "A"); prepend_right(5, "B"); => "BA" appears after position 5
    pub fn prepend_right(&mut self, index: u32, content: &str) -> &mut Self {
        if !content.is_empty() {
            let content_ref = self.allocator.alloc_str(content);
            let insert_idx = self.find_insert_position_for_prepend(index, true);
            self.chunks.insert(insert_idx, Chunk::inserted(content_ref));
        }
        self
    }

    /// Append content after a specific position (inserts after the position).
    /// Multiple append_right calls at the same index stack in order (first call first).
    /// Example: append_right(5, "A"); append_right(5, "B"); => "AB" appears after position 5
    pub fn append_right(&mut self, index: u32, content: &str) -> &mut Self {
        if !content.is_empty() {
            let content_ref = self.allocator.alloc_str(content);
            let insert_idx = self.find_insert_position_for_append(index, true);
            self.chunks.insert(insert_idx, Chunk::inserted(content_ref));
        }
        self
    }

    /// Split chunks at the given index if needed, returns the index where the split occurred
    fn ensure_split_at(&mut self, index: u32) {
        for i in 0..self.chunks.len() {
            if let Chunk::Original { start, end } = self.chunks[i] {
                if start < index && index < end {
                    // Split this chunk into two
                    let first = Chunk::from_source(start, index);
                    let second = Chunk::from_source(index, end);
                    self.chunks[i] = first;
                    self.chunks.insert(i + 1, second);
                    return;
                }
            }
        }
    }

    /// Find position for prepend (inserts BEFORE existing insertions at same position)
    fn find_insert_position_for_prepend(&mut self, index: u32, after: bool) -> usize {
        self.ensure_split_at(index);

        for (i, chunk) in self.chunks.iter().enumerate() {
            match chunk {
                Chunk::Original { start, .. } => {
                    if *start == index {
                        return if after { i + 1 } else { i };
                    }
                    if *start > index {
                        return i;
                    }
                }
                // Edited chunks with original positions should be treated like Original chunks
                Chunk::Edited {
                    original_start: Some(s),
                    was_moved: false,
                    ..
                } => {
                    if *s == index {
                        return if after { i + 1 } else { i };
                    }
                    if *s > index {
                        return i;
                    }
                }
                // Pure insertions and moved chunks - skip
                Chunk::Edited { .. } => {}
            }
        }

        self.chunks.len()
    }

    /// Find position for append (inserts AFTER existing insertions at same position)
    fn find_insert_position_for_append(&mut self, index: u32, after: bool) -> usize {
        self.ensure_split_at(index);

        let mut result = self.chunks.len();

        for (i, chunk) in self.chunks.iter().enumerate() {
            match chunk {
                Chunk::Original { start, .. } => {
                    if *start == index {
                        // For append with after=false, we want to insert right before this Original chunk
                        // but after any existing Edited chunks that precede it
                        if !after {
                            result = i;
                            // Continue scanning to skip past any Edited chunks without original_start
                            // that should come before us
                            break;
                        } else {
                            // after=true means we want to insert after this position
                            // Skip past this chunk and any following Edited chunks
                            result = i + 1;
                            // Continue to find the end of Edited chunks at this position
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
                    if *start > index {
                        return i;
                    }
                }
                // Edited chunks with original positions should be treated like Original chunks
                // for position finding purposes (they occupy the same logical position)
                Chunk::Edited {
                    original_start: Some(s),
                    was_moved: false,
                    ..
                } => {
                    if *s == index {
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
                    if *s > index {
                        return i;
                    }
                }
                // Pure insertions - append goes after these
                Chunk::Edited {
                    original_start: None,
                    ..
                } => {
                    result = i + 1;
                }
                // Moved chunks - skip them as they no longer occupy their original position
                Chunk::Edited {
                    was_moved: true, ..
                } => {}
            }
        }

        result
    }

    /// Overwrite a range with new content
    pub fn overwrite(&mut self, start: u32, end: u32, content: &str) -> &mut Self {
        if start >= end {
            return self;
        }

        let content_ref = self.allocator.alloc_str(content);
        let mut new_chunks = Vec::new();
        let mut handled = false;

        for chunk in self.chunks.drain(..) {
            match chunk {
                Chunk::Original {
                    start: chunk_start,
                    end: chunk_end,
                } => {
                    if chunk_end <= start {
                        new_chunks.push(Chunk::Original {
                            start: chunk_start,
                            end: chunk_end,
                        });
                        continue;
                    }

                    if chunk_start >= end {
                        if !handled {
                            new_chunks.push(Chunk::overwritten(start, end, content_ref));
                            handled = true;
                        }
                        new_chunks.push(Chunk::Original {
                            start: chunk_start,
                            end: chunk_end,
                        });
                        continue;
                    }

                    if !handled {
                        if chunk_start < start {
                            new_chunks.push(Chunk::from_source(chunk_start, start));
                        }

                        new_chunks.push(Chunk::overwritten(start, end, content_ref));
                        handled = true;

                        if chunk_end > end {
                            new_chunks.push(Chunk::from_source(end, chunk_end));
                        }
                    } else {
                        // Chunk overlaps with already-handled overwrite range
                        // Preserve content after the overwrite end
                        if chunk_end > end {
                            new_chunks.push(Chunk::from_source(end, chunk_end));
                        }
                    }
                }
                Chunk::Edited {
                    original_start,
                    original_end,
                    content: old_content,
                    was_moved,
                } => {
                    // Moved chunks should be passed through without affecting overwrite position
                    // Their original positions don't reflect their current location in the output
                    if was_moved {
                        new_chunks.push(Chunk::Edited {
                            original_start,
                            original_end,
                            content: old_content,
                            was_moved,
                        });
                        continue;
                    }

                    let (chunk_start, chunk_end) = match (original_start, original_end) {
                        (Some(s), Some(e)) => (s, e),
                        _ => {
                            new_chunks.push(Chunk::Edited {
                                original_start,
                                original_end,
                                content: old_content,
                                was_moved,
                            });
                            continue;
                        }
                    };

                    if chunk_end <= start {
                        new_chunks.push(Chunk::Edited {
                            original_start,
                            original_end,
                            content: old_content,
                            was_moved,
                        });
                        continue;
                    }

                    if chunk_start >= end {
                        if !handled {
                            new_chunks.push(Chunk::overwritten(start, end, content_ref));
                            handled = true;
                        }
                        new_chunks.push(Chunk::Edited {
                            original_start,
                            original_end,
                            content: old_content,
                            was_moved,
                        });
                        continue;
                    }

                    if !handled {
                        new_chunks.push(Chunk::overwritten(start, end, content_ref));
                        handled = true;
                    }
                    // Note: For Edited chunks that overlap with already-handled overwrite range,
                    // we drop them entirely since we can't easily preserve their suffix content
                    // (the content is already transformed and may not map cleanly to positions).
                    // The key fix for script content is in the Original chunk handling above.
                }
            }
        }

        if !handled {
            new_chunks.push(Chunk::overwritten(start, end, content_ref));
        }

        self.chunks = new_chunks;
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
    /// This removes content from `start..end` and inserts it at `target_index`.
    /// The moved content will map back to its original source position.
    /// Any inserted content (via prepend_left, append_right, etc.) within the range
    /// will also be moved along with the original content.
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

        // Ensure splits at boundaries
        self.ensure_split_at(start);
        self.ensure_split_at(end);

        // First pass: identify which chunks to move based on their positions
        // For Original and Edited with original_start, we use those positions.
        // For pure insertions, we track position based on the preceding chunk.
        let mut indices_to_move = Vec::new();
        let mut current_pos = 0u32;

        for (i, chunk) in self.chunks.iter().enumerate() {
            match chunk {
                Chunk::Original { start: cs, end: ce } => {
                    // Check if this Original chunk is within the move range
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
                    // For already-moved chunks, check based on current_pos (their insertion point)
                    // rather than original position, so they get included in subsequent moves
                    // that encompass where they were inserted
                    if *was_moved {
                        if current_pos >= start && current_pos < end {
                            indices_to_move.push(i);
                        }
                        // Don't update current_pos - moved chunks don't occupy original positions
                        continue;
                    }
                    // Edited chunk with original position - check if within range
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
                    // Pure insertion - its position is the end of the previous chunk
                    // Move it if that position is within the range (exclusive of end)
                    if current_pos >= start && current_pos < end {
                        indices_to_move.push(i);
                    }
                    // Pure insertions don't change current_pos
                }
                Chunk::Edited {
                    original_start: Some(os),
                    original_end: None,
                    was_moved,
                    ..
                } => {
                    // For already-moved chunks, check based on current_pos
                    if *was_moved {
                        if current_pos >= start && current_pos < end {
                            indices_to_move.push(i);
                        }
                        continue;
                    }
                    // Partial position info - use original_start for position check
                    if *os >= start && *os < end {
                        indices_to_move.push(i);
                    }
                }
                Chunk::Edited {
                    original_start: None,
                    original_end: Some(_),
                    ..
                } => {
                    // Partial position info - treat like pure insertion
                    if current_pos >= start && current_pos < end {
                        indices_to_move.push(i);
                    }
                }
            }
        }

        if indices_to_move.is_empty() {
            return self;
        }

        // Collect chunks to move, converting Original to Moved and marking Edited as moved
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
                    // Mark as moved so prepend_left will skip this chunk
                    chunks_to_move.push(Chunk::Edited {
                        original_start,
                        original_end,
                        content,
                        was_moved: true,
                    });
                }
            }
        }

        // Remove chunks in reverse order to maintain indices
        for &i in indices_to_move.iter().rev() {
            self.chunks.remove(i);
        }

        // Find insert position for target - always insert at the left of target position
        // (so moved content starts at target_index in the original document layout)
        let insert_idx = self.find_insert_position_for_append(target_index, false);

        // Insert all moved chunks at the target position
        let insert_pos = insert_idx.min(self.chunks.len());
        for (i, chunk) in chunks_to_move.into_iter().enumerate() {
            self.chunks.insert(insert_pos + i, chunk);
        }

        self
    }

    /// Move a slice with an unmapped prefix before it.
    ///
    /// The prefix is NOT source-mapped. Only the moved content maintains its original mapping.
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
    /// The suffix is NOT source-mapped. Only the moved content maintains its original mapping.
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
    /// The prefix and suffix are NOT source-mapped. Only the moved content maintains
    /// its original mapping. This is useful for wrapping moved content like `{moved_text}`.
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

        // Ensure splits at boundaries
        self.ensure_split_at(start);
        self.ensure_split_at(end);

        // First pass: identify which chunks to move based on their positions
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
                    // For already-moved chunks, check based on current_pos (their insertion point)
                    if *was_moved {
                        if current_pos >= start && current_pos < end {
                            indices_to_move.push(i);
                        }
                        // Don't update current_pos - moved chunks don't occupy original positions
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
                    // For already-moved chunks, check based on current_pos
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

        // Collect chunks to move, converting Original to Moved and marking Edited as moved
        let mut chunks_to_move = Vec::new();

        // Add prefix as unmapped insertion if present
        if !prefix.is_empty() {
            let prefix_ref = self.allocator.alloc_str(prefix);
            chunks_to_move.push(Chunk::inserted(prefix_ref));
        }

        // Add the actual moved chunks
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
                    // Mark as moved so prepend_left will skip this chunk
                    chunks_to_move.push(Chunk::Edited {
                        original_start,
                        original_end,
                        content,
                        was_moved: true,
                    });
                }
            }
        }

        // Add suffix as unmapped insertion if present
        if !suffix.is_empty() {
            let suffix_ref = self.allocator.alloc_str(suffix);
            chunks_to_move.push(Chunk::inserted(suffix_ref));
        }

        // Remove chunks in reverse order to maintain indices
        for &i in indices_to_move.iter().rev() {
            self.chunks.remove(i);
        }

        // Find insert position for target - always insert at the left of target position
        // (so moved content starts at target_index in the original document layout)
        let insert_idx = self.find_insert_position_for_append(target_index, false);

        // Insert all moved chunks at the target position
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

    /// Get the intro text
    pub(super) fn intro(&self) -> &str {
        self.intro
    }

    /// Get the outro text
    pub(super) fn outro(&self) -> &str {
        self.outro
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
