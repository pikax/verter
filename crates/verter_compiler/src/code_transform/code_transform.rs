use std::cell::OnceCell;

use rustc_hash::FxHashMap;
use smallvec::SmallVec;

use super::chunk::Chunk;
use crate::cursor::position::PositionResolver;
use oxc_allocator::Allocator;

/// Result of scanning chunks for a target position.
#[allow(dead_code)] // Used by move_slice() which is test/API only for now
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
/// ```ignore
/// use verter_compiler::code_transform::CodeTransform;
/// use oxc_allocator::Allocator;
///
/// let allocator = Allocator::default();
/// let mut ct = CodeTransform::new("Hello World", &allocator);
/// ct.overwrite(6, 11, "Rust");
/// assert_eq!(ct.build_string(), "Hello Rust");
/// ```
pub struct CodeTransform<'a> {
    /// The original source text (never modified)
    pub(super) original: &'a str,
    /// List of chunks representing the output content.
    /// Positioned chunks (Original and Overwritten) maintain monotonically
    /// increasing source positions.
    pub(super) chunks: Vec<Chunk<'a>>,
    /// Scratch buffer for batch operations. Swapped with `chunks` to avoid
    /// allocating a new Vec on each batch call — after the first batch op,
    /// both Vecs retain their capacity.
    pub(super) scratch: Vec<Chunk<'a>>,
    /// Content to prepend before everything
    intro: &'a str,
    /// Content to append after everything
    outro: &'a str,
    /// The bump allocator for string allocations
    pub(super) allocator: &'a Allocator,
    /// Cursor hint: last known chunk index for a given position.
    /// Used to accelerate forward-progressing access patterns.
    pub(super) cursor_hint: usize,
    /// Running delta between output length and original length (excluding intro/outro).
    /// Tracked incrementally by each mutation to avoid a full scan in build_string().
    pub(super) output_delta: i64,
    /// Lazily-built position resolver for the original source, shared across
    /// every source map produced from this transform. Built once on first map
    /// demand (one resolver per original source), never rebuilt per map.
    resolver: OnceCell<PositionResolver<'a>>,
    /// Explicit original-source offsets that must receive source-map segments
    /// in addition to ordinary chunk and line starts.
    pub(super) sourcemap_locations: Vec<u32>,
    /// Exact unmapped transitions inside bump-allocated generated chunks.
    /// Pointer identity makes markers disappear naturally when a later edit
    /// removes their owning chunk.
    generated_unmapped_boundaries: FxHashMap<(usize, usize), SmallVec<[u32; 2]>>,
    /// Test-only: the token-buffer capacity reserved by the most recent
    /// `generate_map`, captured at the reservation point. Lets tests assert the
    /// reservation covers every emitted token (no reallocation during
    /// population) directly against the production map path.
    #[cfg(test)]
    last_reserved_token_capacity: std::cell::Cell<usize>,
    /// The exact content reference of the leading helper-import preamble insertion, when one was
    /// recorded (IDE script codegen, via `CodeGenOutput::prepend_helper_preamble`). Identity is by
    /// bump-allocated pointer: the same `&'a str` flows from the insertion into its `Inserted`
    /// chunk, so source-map generation can locate it and report the generated-TSX position
    /// immediately AFTER it — the typed helper-import-preamble end boundary. `None` when no
    /// preamble was recorded (non-IDE transforms, or codegen that emitted no helper imports).
    helper_preamble_content: Option<&'a str>,
    /// Whether any affinity-anchored insertion (from the checked insertion
    /// operations) exists. Gates the boundary-attachment passes in the range
    /// replacements so transforms that only use the positional insertion API
    /// keep their historical code path with zero extra work.
    pub(super) anchored_present: bool,
}

/// One source-bearing output range from a [`CodeTransform`]. This crate-private
/// projection lets higher-level emitters preserve transform provenance while
/// composing several transformed fragments into a larger generated artifact.
/// Pure insertions are intentionally absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GeneratedSourceRange {
    /// Start byte in the built output.
    pub(crate) generated_start: u32,
    /// End byte in the built output.
    pub(crate) generated_end: u32,
    /// Start byte in the transform's original source.
    pub(crate) source_start: u32,
    /// Whether the range is replacement text rather than byte-preserved source.
    /// Replacement ranges require the caller's typed token mapping because the
    /// replacement and authored bytes do not have a general linear relation.
    pub(crate) replacement: bool,
}

fn push_preserved_source_ranges(
    ranges: &mut Vec<GeneratedSourceRange>,
    locations: &[u32],
    generated_start: u32,
    source_start: u32,
    len: u32,
) {
    let source_end = source_start.saturating_add(len);
    let mut segment_source = source_start;
    let mut segment_generated = generated_start;
    for location in locations
        .iter()
        .copied()
        .filter(|location| *location > source_start && *location < source_end)
    {
        let segment_len = location - segment_source;
        ranges.push(GeneratedSourceRange {
            generated_start: segment_generated,
            generated_end: segment_generated + segment_len,
            source_start: segment_source,
            replacement: false,
        });
        segment_source = location;
        segment_generated += segment_len;
    }
    if segment_source < source_end {
        ranges.push(GeneratedSourceRange {
            generated_start: segment_generated,
            generated_end: generated_start + len,
            source_start: segment_source,
            replacement: false,
        });
    }
}

#[allow(dead_code)] // Many API methods only exercised by tests currently
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
            resolver: OnceCell::new(),
            sourcemap_locations: Vec::new(),
            generated_unmapped_boundaries: FxHashMap::default(),
            #[cfg(test)]
            last_reserved_token_capacity: std::cell::Cell::new(0),
            helper_preamble_content: None,
            anchored_present: false,
        }
    }

    /// Get the original source text
    pub fn original(&self) -> &str {
        self.original
    }

    /// Position resolver for the original source, built once and shared across
    /// every source map generated from this transform. Uses the
    /// sourcemap-optimized constructor that skips the UTF-16 cumulative offset
    /// cache (only line/column are needed here).
    pub(super) fn sourcemap_resolver(&self) -> &PositionResolver<'a> {
        self.resolver
            .get_or_init(|| PositionResolver::new_for_sourcemap(self.original))
    }

    /// Test accessor: the cached resolver, or `None` if no map has demanded it
    /// yet. Used to assert lazy, build-once-and-reuse semantics.
    #[cfg(test)]
    pub(crate) fn sourcemap_resolver_for_test(&self) -> Option<&PositionResolver<'a>> {
        self.resolver.get()
    }

    /// Record the source-map token-buffer capacity reserved by `generate_map`,
    /// captured at the reservation point. Called only from the map path under
    /// test configuration.
    #[cfg(test)]
    pub(super) fn record_reserved_token_capacity(&self, capacity: usize) {
        self.last_reserved_token_capacity.set(capacity);
    }

    /// Test accessor: the token-buffer capacity reserved by the most recent
    /// `generate_map`. Used to assert the reservation covers every emitted
    /// token, so the buffer never reallocates during population.
    #[cfg(test)]
    pub(crate) fn last_reserved_token_capacity_for_test(&self) -> usize {
        self.last_reserved_token_capacity.get()
    }

    /// Record the content reference of the helper-import preamble insertion (called by
    /// [`CodeGenOutput::apply_to`](crate::template::code_gen::types::CodeGenOutput::apply_to) after
    /// the IDE script codegen prepends the preamble). The same `&'a str` ends up as an `Inserted`
    /// chunk, so [`generate_map_with_preamble`](Self::generate_map_with_preamble) can locate it by
    /// pointer identity and report the generated-TSX position immediately after it.
    pub fn set_helper_preamble_content(&mut self, content: &'a str) {
        self.helper_preamble_content = Some(content);
    }

    /// The recorded helper-import preamble insertion content, if any. Used by source-map
    /// generation to compute the typed preamble-end boundary.
    pub(super) fn helper_preamble_content(&self) -> Option<&'a str> {
        self.helper_preamble_content
    }

    /// Allocate a string in the bump allocator, returning a reference with the
    /// same lifetime as the CodeTransform. Useful for deferring insertions.
    pub fn alloc_str(&self, s: &str) -> &'a str {
        self.allocator.alloc_str(s)
    }

    /// Single audit-emit point for one `CodeTransform` operation. Called
    /// at the entry of every public op (prepend / append / overwrite /
    /// remove / move / batch) to populate
    /// [`verter_audit::payloads::compile::CompilePayload::code_transform_ops`].
    /// `current_observer()` returns `None` outside an audited request,
    /// so this is a single TLS load with no allocation on the hot path.
    /// The audit guard `audit_no_hot_loop_instrumentation` must keep
    /// callers off the per-element / per-attribute inner loops; this
    /// method itself is the boundary that satisfies the per-op contract.
    #[inline]
    pub(super) fn record_audit_op(&self) {
        if let Some(observer) = verter_audit::current_observer() {
            observer.record_event(verter_audit::AuditEvent::CompileCodeTransformOp);
        }
    }

    /// Prepend content to the very start
    pub fn prepend(&mut self, content: &str) -> &mut Self {
        self.record_audit_op();
        if !content.is_empty() {
            let mut buf = String::with_capacity(content.len() + self.intro.len());
            buf.push_str(content);
            buf.push_str(self.intro);
            self.intro = self.allocator.alloc_str(&buf);
        }
        self
    }

    /// Prepend `content` as the unmapped intro (output offset 0) AND record it as the
    /// helper-import preamble, so source-map generation publishes the
    /// `x_verter_helper_preamble_end` boundary at the generated position immediately after the
    /// intro (see [`generate_map_with_preamble`](Self::generate_map_with_preamble)).
    ///
    /// This is the single-prepend, empty-intro contract: the boundary "end of intro" equals "end
    /// of helper preamble" ONLY when the intro IS exactly the preamble, so an EMPTY intro is
    /// required (debug-asserted). It records the STORED intro pointer — NOT the `content` argument
    /// — because [`prepend`](Self::prepend) re-allocates the intro as `alloc_str(content + old_intro)`,
    /// so the bump-allocated `&'a str` source-map generation reads via `self.intro()` is a fresh
    /// pointer that pointer-matches the recorded preamble only when read back from the field. The
    /// Svelte IDE projector uses this to keep its `@jsxImportSource`-led prelude as the leading
    /// output bytes while still publishing the typed preamble-end boundary.
    pub fn prepend_helper_preamble_content(&mut self, content: &str) -> &mut Self {
        // The boundary "end of intro" equals "end of helper preamble" ONLY when the intro IS exactly
        // the preamble — i.e. the intro was empty before this prepend. The debug-assert catches a
        // contract violation loudly in test/CI builds; the runtime check makes release builds FAIL
        // CLOSED (degrade to boundary-absent, which `generate_map_with_preamble`'s consumers already
        // handle) rather than record a combined `content + old_intro` pointer that would publish a
        // WRONG boundary after the whole combined intro.
        let intro_was_empty = self.intro.is_empty();
        debug_assert!(
            intro_was_empty,
            "prepend_helper_preamble_content requires an empty intro (single-prepend contract)"
        );
        self.prepend(content);
        if intro_was_empty {
            // The intro now IS exactly `content` (a fresh bump alloc; old intro empty). Record that
            // STORED pointer — NOT the `content` argument — because `prepend` re-allocated it, and
            // `generate_map_with_preamble` matches the preamble by `self.intro()` pointer identity.
            self.set_helper_preamble_content(self.intro);
        }
        self
    }

    /// Append content to the very end
    pub fn append(&mut self, content: &str) -> &mut Self {
        self.record_audit_op();
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
        self.record_audit_op();
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
        self.record_audit_op();
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
        self.record_audit_op();
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
        self.record_audit_op();
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
            Chunk::Inserted { .. }
            | Chunk::Moved { .. }
            | Chunk::InsertedMapped { .. }
            | Chunk::InsertedAnchored { .. } => None,
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
    pub(super) fn search_start_for(&self, index: u32) -> usize {
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
                Chunk::Inserted { .. }
                | Chunk::Moved { .. }
                | Chunk::InsertedMapped { .. }
                | Chunk::InsertedAnchored { .. } => {}
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
            if matches!(
                &self.chunks[j],
                Chunk::Inserted { .. }
                    | Chunk::InsertedMapped { .. }
                    | Chunk::InsertedAnchored { .. }
            ) {
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
                Chunk::Inserted { .. }
                | Chunk::Moved { .. }
                | Chunk::InsertedMapped { .. }
                | Chunk::InsertedAnchored { .. } => {}
            }
        }
        false
    }

    /// Overwrite a range with new content.
    ///
    /// Clears any boundary insertions attached to the replaced range by the
    /// checked insertion operations (see [`update`](Self::update) for the
    /// content-only twin that preserves them). Explicitly unchecked: offsets
    /// are trusted, matching the historical behavior for every existing
    /// caller; use [`try_overwrite`](Self::try_overwrite) for typed refusals
    /// of malformed offsets.
    pub fn overwrite(&mut self, start: u32, end: u32, content: &str) -> &mut Self {
        self.record_audit_op();
        if start >= end {
            return self;
        }

        // Avoid bump-allocating empty strings — use a static empty slice.
        let content_ref = if content.is_empty() {
            ""
        } else {
            self.allocator.alloc_str(content)
        };

        self.replace_range_impl(start, end, content_ref, false);
        self
    }

    /// Overwrite a range while retaining exact unmapped source-map
    /// transitions inside the replacement text. Offsets are UTF-8 byte
    /// boundaries relative to `content`; malformed offsets are ignored.
    pub(crate) fn overwrite_with_unmapped_boundaries(
        &mut self,
        start: u32,
        end: u32,
        content: &str,
        boundaries: &[u32],
    ) -> &mut Self {
        self.record_audit_op();
        if start >= end {
            return self;
        }
        let content_ref = if content.is_empty() {
            ""
        } else {
            self.allocator.alloc_str(content)
        };
        self.record_generated_unmapped_boundaries(content_ref, boundaries);
        self.replace_range_impl(start, end, content_ref, false);
        self
    }

    /// Insert generated text before `index` and retain exact unmapped
    /// source-map transitions inside that insertion.
    pub(crate) fn prepend_left_with_unmapped_boundaries(
        &mut self,
        index: u32,
        content: &str,
        boundaries: &[u32],
    ) -> &mut Self {
        self.record_audit_op();
        if content.is_empty() {
            return self;
        }
        let content_ref = self.allocator.alloc_str(content);
        self.record_generated_unmapped_boundaries(content_ref, boundaries);
        let insert_idx = self.find_insert_position_for_prepend(index, false);
        self.chunks.insert(insert_idx, Chunk::inserted(content_ref));
        if insert_idx <= self.cursor_hint {
            self.cursor_hint += 1;
        }
        self.output_delta += content.len() as i64;
        self
    }

    fn record_generated_unmapped_boundaries(&mut self, content: &'a str, boundaries: &[u32]) {
        let offsets = boundaries
            .iter()
            .copied()
            .filter(|offset| {
                (*offset as usize) < content.len() && content.is_char_boundary(*offset as usize)
            })
            .collect::<SmallVec<[u32; 2]>>();
        if !offsets.is_empty() {
            self.generated_unmapped_boundaries
                .entry((content.as_ptr() as usize, content.len()))
                .or_default()
                .extend(offsets);
        }
    }

    /// Content-only replacement: overwrite the range's content while
    /// PRESERVING the boundary insertions the checked insertion operations
    /// attached to the range (RIGHT-affinity content at `start`, and the
    /// first chunk's end-boundary LEFT-affinity content), clearing interior
    /// insertions. This is the `magic-string` `update` vs `overwrite`
    /// distinction; for insertions made through the positional API the two
    /// operations coincide.
    ///
    /// Explicitly unchecked like [`overwrite`](Self::overwrite) (zero-length
    /// ranges are a silent no-op; offsets are trusted); use
    /// [`try_update`](Self::try_update) for typed refusals.
    pub fn update(&mut self, start: u32, end: u32, content: &str) -> &mut Self {
        self.record_audit_op();
        if start >= end {
            return self;
        }

        let content_ref = if content.is_empty() {
            ""
        } else {
            self.allocator.alloc_str(content)
        };

        self.replace_range_impl(start, end, content_ref, true);
        self
    }

    /// The splice engine shared by every range replacement: replaces the
    /// positioned chunks covering `[start, end)` with a single `Overwritten`
    /// chunk (plus preserved partial edges), removing free-standing insertion
    /// chunks strictly inside the range. Returns `false` when the
    /// nested-overwrite no-op fired (a strictly larger replaced range already
    /// covers this one — nothing changed), `true` otherwise.
    /// Boundary-affinity handling lives in
    /// [`replace_range_impl`](Self::replace_range_impl).
    pub(super) fn splice_replace_range(
        &mut self,
        start: u32,
        end: u32,
        content_ref: &'a str,
    ) -> bool {
        // Fast path: single Original chunk contains the entire range — every
        // replaced byte is live original text, so the delta is exact.
        if self.try_fast_overwrite(start, end, content_ref) {
            self.output_delta += content_ref.len() as i64 - (end - start) as i64;
            return true;
        }

        let search_start = self.search_start_for(start);
        let mut first_affected: Option<usize> = None;
        let mut last_affected: Option<usize> = None;
        let mut replacement_chunks: SmallVec<[Chunk<'a>; 4]> = SmallVec::new();
        let mut handled = false;
        // The output bytes the splice actually deletes — LIVE original text
        // still covered by `Original` chunks plus replacement content of
        // subsumed `Overwritten` chunks. Counting per affected chunk (never
        // the nominal `end - start`) keeps overlapping edits from
        // double-charging bytes an earlier edit already removed, which would
        // drive `original.len() + output_delta` negative and wrap
        // `build_string`'s capacity.
        let mut removed_output: i64 = 0;

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
                    // Live original bytes inside the range.
                    removed_output +=
                        i64::from(chunk_end.min(end)) - i64::from(chunk_start.max(start));

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
                    content,
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
                        // Nothing was charged yet — the no-op changes nothing.
                        return false;
                    }

                    if first_affected.is_none() {
                        first_affected = Some(i);
                    }
                    last_affected = Some(i);
                    // The subsumed chunk's replacement content leaves the
                    // output (its ORIGINAL bytes were already charged by the
                    // edit that produced it).
                    removed_output += content.len() as i64;

                    if !handled {
                        replacement_chunks.push(Chunk::overwritten(start, end, content_ref));
                        handled = true;
                    }
                }
                // Moved and pure-insertion chunks don't participate in overwrite resolution
                Chunk::Moved { .. }
                | Chunk::Inserted { .. }
                | Chunk::InsertedMapped { .. }
                | Chunk::InsertedAnchored { .. } => {
                    continue;
                }
            }
        }

        if let (Some(first), Some(last)) = (first_affected, last_affected) {
            // Insertion-family chunks positioned between the affected chunks
            // are spliced out with them — their content leaves the output.
            for chunk in &self.chunks[first..=last] {
                match chunk {
                    Chunk::Moved { content, .. }
                    | Chunk::Inserted { content }
                    | Chunk::InsertedMapped { content, .. }
                    | Chunk::InsertedAnchored { content, .. } => {
                        removed_output += content.len() as i64;
                    }
                    Chunk::Original { .. } | Chunk::Overwritten { .. } => {}
                }
            }
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

        self.output_delta += content_ref.len() as i64 - removed_output;
        true
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
    /// ```ignore
    /// use verter_compiler::code_transform::CodeTransform;
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
    /// ```ignore
    /// use verter_compiler::code_transform::CodeTransform;
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
    /// ```ignore
    /// use verter_compiler::code_transform::CodeTransform;
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
    /// ```ignore
    /// use verter_compiler::code_transform::CodeTransform;
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
        self.record_audit_op();
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
                Chunk::Moved { .. }
                | Chunk::Inserted { .. }
                | Chunk::InsertedMapped { .. }
                | Chunk::InsertedAnchored { .. } => {
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
                    chunks_to_move.push(Chunk::moved_replacement(start, end, content));
                }
                Chunk::Moved {
                    start,
                    end,
                    content,
                    replacement,
                    ..
                } => {
                    chunks_to_move.push(if replacement {
                        Chunk::moved_replacement(start, end, content)
                    } else {
                        Chunk::moved(start, end, content)
                    });
                }
                Chunk::Inserted { content } => {
                    chunks_to_move.push(Chunk::inserted(content));
                }
                Chunk::InsertedMapped { content, .. } => {
                    // When moved, InsertedMapped loses its source mapping
                    // (the mapping was relative to the original insertion site)
                    chunks_to_move.push(Chunk::inserted(content));
                }
                Chunk::InsertedAnchored { content, .. } => {
                    // When moved, an anchored insertion loses its boundary
                    // attachment (the anchor was relative to the original
                    // insertion site) and travels as a plain insertion.
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

    pub(super) fn sourcemap_locations(&self) -> &[u32] {
        &self.sourcemap_locations
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

    // Batch chunk-rebuild operations (`batch_prepend_left_static`,
    // `batch_prepend_left_with_source_map`, `batch_prepend_left_merged`,
    // `batch_overwrite`) live in the sibling `batch_ops` module.
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
                | Chunk::Moved { content, .. }
                | Chunk::InsertedMapped { content, .. }
                | Chunk::InsertedAnchored { content, .. } => {
                    out.push_str(content);
                }
            }
        }
        out.push_str(self.outro);
        out
    }

    /// Build the output and retain the source-bearing range geometry.
    ///
    /// This is deliberately not a second source-map implementation. It exposes
    /// the same chunk authority used by [`Self::generate_map`] so a compound
    /// emitter can relocate transformed fragments and lower the final artifact
    /// through one ordinary `CodeTransform` map. Synthesized insertions are not
    /// represented; callers therefore cannot accidentally assign them authored
    /// provenance.
    pub(crate) fn build_string_with_source_ranges(&self) -> (String, Vec<GeneratedSourceRange>) {
        let (output, ranges, _) = self.build_string_with_source_ranges_and_unmapped_boundaries();
        (output, ranges)
    }

    /// Build the output, authored range geometry, and explicit ownership
    /// transitions inside generated chunks.
    pub(crate) fn build_string_with_source_ranges_and_unmapped_boundaries(
        &self,
    ) -> (String, Vec<GeneratedSourceRange>, Vec<u32>) {
        let output = self.build_string();
        let mut generated = self.intro.len() as u32;
        let mut ranges = Vec::with_capacity(self.chunks.len());
        let mut unmapped_boundaries = Vec::new();
        let mut locations = self.sourcemap_locations.clone();
        locations.sort_unstable();
        locations.dedup();
        for chunk in &self.chunks {
            let content = match chunk {
                Chunk::Original { .. } => None,
                Chunk::Inserted { content }
                | Chunk::Overwritten { content, .. }
                | Chunk::Moved { content, .. }
                | Chunk::InsertedMapped { content, .. }
                | Chunk::InsertedAnchored { content, .. } => Some(*content),
            };
            if let Some(content) = content {
                if let Some(boundaries) = self
                    .generated_unmapped_boundaries
                    .get(&(content.as_ptr() as usize, content.len()))
                {
                    unmapped_boundaries.extend(boundaries.iter().map(|offset| generated + offset));
                }
            }
            match chunk {
                Chunk::Original { start, end } => {
                    let len = end - start;
                    if len > 0 {
                        push_preserved_source_ranges(
                            &mut ranges,
                            &locations,
                            generated,
                            *start,
                            len,
                        );
                    }
                    generated += len;
                }
                Chunk::Moved {
                    start,
                    content,
                    replacement,
                    ..
                } => {
                    let len = content.len() as u32;
                    if len > 0 {
                        if *replacement {
                            ranges.push(GeneratedSourceRange {
                                generated_start: generated,
                                generated_end: generated + len,
                                source_start: *start,
                                replacement: true,
                            });
                        } else {
                            push_preserved_source_ranges(
                                &mut ranges,
                                &locations,
                                generated,
                                *start,
                                len,
                            );
                        }
                    }
                    generated += len;
                }
                Chunk::Overwritten { start, content, .. } => {
                    let len = content.len() as u32;
                    if len > 0 {
                        ranges.push(GeneratedSourceRange {
                            generated_start: generated,
                            generated_end: generated + len,
                            source_start: *start,
                            replacement: true,
                        });
                    }
                    generated += len;
                }
                Chunk::InsertedMapped {
                    content,
                    source_start,
                    content_offset,
                } => {
                    let len = content.len() as u32;
                    let offset = (*content_offset).min(len);
                    if offset < len {
                        ranges.push(GeneratedSourceRange {
                            generated_start: generated + offset,
                            generated_end: generated + len,
                            source_start: *source_start,
                            replacement: true,
                        });
                    }
                    generated += len;
                }
                Chunk::Inserted { content } | Chunk::InsertedAnchored { content, .. } => {
                    generated += content.len() as u32;
                }
            }
        }
        debug_assert_eq!(
            generated as usize + self.outro.len(),
            output.len(),
            "source-range geometry must cover the built output bytes"
        );
        unmapped_boundaries.sort_unstable();
        unmapped_boundaries.dedup();
        (output, ranges, unmapped_boundaries)
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
                | Chunk::Moved { content, .. }
                | Chunk::InsertedMapped { content, .. }
                | Chunk::InsertedAnchored { content, .. } => {
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
