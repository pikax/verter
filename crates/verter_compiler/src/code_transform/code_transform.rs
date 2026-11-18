use std::cell::OnceCell;

use rustc_hash::{FxHashMap, FxHashSet};
use smallvec::SmallVec;

use super::chunk::{Chunk, LeafChunks};
use super::fallible::CodeTransformError;
use super::freeze::chunk_content;
use crate::cursor::position::PositionResolver;
use crate::emission::{note_map_capture, MapMode, Piece, Provenance};
use oxc_allocator::Allocator;
use verter_span::Span;

#[cfg(test)]
thread_local! {
    static CODE_TRANSFORM_CONSTRUCTIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static CODE_TRANSFORM_BUILD_STRING_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static CODE_TRANSFORM_CHUNK_VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn code_transform_construction_count() -> usize {
    CODE_TRANSFORM_CONSTRUCTIONS.with(std::cell::Cell::get)
}

#[cfg(test)]
pub(crate) fn reset_code_transform_construction_count() {
    CODE_TRANSFORM_CONSTRUCTIONS.with(|count| count.set(0));
}

#[cfg(test)]
pub(crate) fn code_transform_build_string_call_count() -> usize {
    CODE_TRANSFORM_BUILD_STRING_CALLS.with(std::cell::Cell::get)
}

#[cfg(test)]
pub(crate) fn reset_code_transform_build_string_call_count() {
    CODE_TRANSFORM_BUILD_STRING_CALLS.with(|count| count.set(0));
}

/// Chunks inspected by position searches and move scans on this thread: the
/// work an edit costs beyond its own output.
#[cfg(test)]
pub(crate) fn take_code_transform_chunk_visits() -> usize {
    CODE_TRANSFORM_CHUNK_VISITS.with(|count| count.replace(0))
}

#[inline]
fn count_chunk_visit() {
    #[cfg(test)]
    CODE_TRANSFORM_CHUNK_VISITS.with(|count| count.set(count.get() + 1));
}

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
pub struct CodeTransform<
    'a,
    M: verter_audit::observe::ObserveExec = verter_audit::observe::Uncaptured,
> {
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
    /// Content to prepend before everything, one piece per call, in REVERSE
    /// output order (the latest prepend renders first). Each call allocates
    /// only its own content.
    intro: Vec<&'a str>,
    /// Total bytes of `intro`.
    intro_len: usize,
    /// Content to append after everything, one piece per call, in output
    /// order.
    outro: Vec<&'a str>,
    /// Total bytes of `outro`.
    outro_len: usize,
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
    /// Content identity (pointer, length) of `Overwritten` chunks created via
    /// [`overwrite_unmapped`](Self::overwrite_unmapped) — wholly synthetic
    /// replacement text with no character-level correspondence to the span
    /// it replaces, so `generate_map` must emit no source-map token for it.
    /// A side-table keyed by content identity (the same pattern as
    /// `generated_unmapped_boundaries`) rather than a new `Chunk::Overwritten`
    /// field: `overwrite_unmapped` reuses the exact same splice engine as
    /// `overwrite` (so every ordering interaction with other operations at
    /// the same position — moves, prepends — is unchanged), differing only
    /// in whether `generate_map` treats the resulting chunk as source-bearing.
    pub(super) unmapped_overwrite_contents: FxHashSet<(usize, usize)>,
    /// Test-only: the token-buffer capacity reserved by the most recent
    /// `generate_map`, captured at the reservation point. Lets tests assert the
    /// reservation covers every emitted token (no reallocation during
    /// population) directly against the production map path.
    #[cfg(test)]
    last_reserved_token_capacity: std::cell::Cell<usize>,
    /// The exact content reference of the leading helper-import preamble insertion, when one was
    /// recorded (IDE script codegen, via `EditRecorder::prepend_helper_preamble`). Identity is by
    /// bump-allocated pointer: the same `&'a str` flows from the insertion into its `Inserted`
    /// chunk, so source-map generation can locate it and report the generated-TSX position
    /// immediately AFTER it — the typed helper-import-preamble end boundary. `None` when no
    /// preamble was recorded (non-IDE transforms, or codegen that emitted no helper imports).
    helper_preamble_content: Option<&'a str>,
    /// Authored insertion point for the leading helper preamble. The preamble
    /// bytes remain synthesized; this separately maps only an engine's
    /// zero-width insertion at the preamble start back to the script start.
    helper_preamble_carrier_anchor: Option<u32>,
    /// Whether any affinity-anchored insertion (from the checked insertion
    /// operations) exists. Gates the boundary-attachment passes in the range
    /// replacements so transforms that only use the positional insertion API
    /// keep their historical code path with zero extra work.
    pub(super) anchored_present: bool,
    /// Optional audit tally of this transform's public operations, published
    /// once when the transform is dropped (see [`AuditOpTally`]).
    audit_ops: AuditOpTally<M>,
    /// Whether map-only facts are captured. Chosen at construction, before any
    /// edit; under [`MapMode::NoMap`] every map product is
    /// [`CodeTransformError::MapNotCaptured`].
    map_mode: MapMode,
}

/// A source length as the `u32` every offset uses, refused before any
/// narrowing.
pub(super) fn check_source_len(len: usize) -> Result<u32, CodeTransformError> {
    u32::try_from(len).map_err(|_| CodeTransformError::Capacity { len })
}

/// Optional per-transform count of `CodeTransform` operations.
///
/// Captured operations bump a plain counter and publish once on drop.
/// Uncaptured operations compile out the counter update and the drop-time
/// observer lookup. The branch is on the mode's constant, so no operation
/// tests a runtime capture flag. Without `semantic-observe` the tally is
/// zero-size and has no drop glue.
#[derive(Debug, Default)]
struct AuditOpTally<M: verter_audit::observe::ObserveExec = verter_audit::observe::Uncaptured> {
    mode: std::marker::PhantomData<M>,
    #[cfg(feature = "semantic-observe")]
    ops: std::cell::Cell<u32>,
}

impl<M: verter_audit::observe::ObserveExec> AuditOpTally<M> {
    #[inline(always)]
    fn bump(&self) {
        #[cfg(feature = "semantic-observe")]
        if M::CAPTURED {
            self.ops.set(self.ops.get().saturating_add(1));
        }
    }
}

#[cfg(feature = "semantic-observe")]
impl<M: verter_audit::observe::ObserveExec> Drop for AuditOpTally<M> {
    fn drop(&mut self) {
        if !M::CAPTURED {
            return;
        }
        let ops = self.ops.get();
        if ops == 0 {
            return;
        }
        verter_audit::observe!(|observer| {
            for _ in 0..ops {
                observer.record_event(verter_audit::AuditEvent::CompileCodeTransformOp);
            }
        });
    }
}

#[cfg(all(test, feature = "semantic-observe"))]
mod audit_tally_tests {
    use super::*;

    #[test]
    fn uncaptured_transform_operations_leave_optional_tally_untouched() {
        let allocator = Allocator::default();
        let mut transform = CodeTransform::new("let n = 1;", &allocator);
        transform.overwrite(8, 9, "2");
        transform.prepend("// value\n");
        assert_eq!(transform.build_string(), "// value\nlet n = 2;");
        assert_eq!(transform.audit_ops.ops.get(), 0);
    }
}

/// One source-bearing output range from a [`CodeTransform`]. This projection
/// lets higher-level emitters preserve transform provenance while composing
/// several transformed fragments into a larger generated artifact, and lets a
/// consumer derive an exact input↔output position mapping from the SAME chunk
/// walk that produced the bytes — never from a second, independently maintained
/// model of the same edits. Pure insertions are intentionally absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GeneratedSourceRange {
    /// Start byte in the built output.
    pub generated_start: u32,
    /// End byte in the built output.
    pub generated_end: u32,
    /// Start byte in the transform's original source.
    pub source_start: u32,
    /// End byte (exclusive) in the transform's original source — the authored
    /// extent this output range accounts for. For a byte-preserved range it is
    /// `source_start + (generated_end - generated_start)`; for a REPLACEMENT it
    /// is the end of the span the replacement text stands in for, which is the
    /// only place the two widths differ.
    pub source_end: u32,
    /// Whether the range is replacement text rather than byte-preserved source.
    /// Replacement ranges require the caller's typed token mapping because the
    /// replacement and authored bytes do not have a general linear relation.
    pub replacement: bool,
}

/// Opaque identity for a byte range inside one generated CodeTransform chunk.
///
/// The content pointer is the registered fact. Consumers resolve it while
/// walking the same chunks that build the output, so authored text equal to
/// the generated bytes can never be mistaken for output geometry.
#[derive(Debug, Clone, Copy)]
pub(crate) struct GeneratedContentMarker<'a> {
    content: &'a str,
    local_start: u32,
    local_end: u32,
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
            source_end: location,
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
            source_end,
            replacement: false,
        });
    }
}

#[allow(dead_code)] // Many API methods only exercised by tests currently
impl<'a> CodeTransform<'a> {
    /// Create an uncaptured transform. Compiler execution selects its mode
    /// explicitly with [`Self::new_in`].
    pub fn new(source: &'a str, allocator: &'a Allocator) -> Self {
        Self::new_in(source, allocator)
    }

    /// An uncaptured transform with an explicit chunk-buffer capacity.
    pub(crate) fn with_chunk_capacity(
        source: &'a str,
        allocator: &'a Allocator,
        estimated_chunks: usize,
    ) -> Self {
        Self::with_chunk_capacity_in(source, allocator, estimated_chunks, MapMode::Map)
    }
}

impl<'a, M: verter_audit::observe::ObserveExec> CodeTransform<'a, M> {
    /// Create a transform compiled for `M`. The compiler passes its selected
    /// execution mode through code generation and source-map construction.
    /// Map facts are captured.
    pub fn new_in(source: &'a str, allocator: &'a Allocator) -> Self {
        Self::with_mode_in(source, allocator, MapMode::Map)
    }

    /// Like [`new_in`](Self::new_in), with the map policy chosen here, before
    /// any edit. Under [`MapMode::NoMap`] no map-only fact is captured.
    pub fn with_mode_in(source: &'a str, allocator: &'a Allocator, mode: MapMode) -> Self {
        let len = source.len();
        // Empirically, kitchen-sink.vue (27370 bytes) produces ~2098 final chunks.
        // Using /13 avoids a Vec reallocation for typical large files.
        let estimated_chunks = if len > 0 { (len / 13).max(64) } else { 0 };
        Self::with_chunk_capacity_in(source, allocator, estimated_chunks, mode)
    }

    /// A transform over `window` of `source`: only the window renders, and
    /// every offset stays absolute in `source`. The chunk buffer is sized
    /// from `edit_hint`, the number of edits the caller expects.
    pub fn for_window_in(
        source: &'a str,
        window: Span,
        edit_hint: usize,
        allocator: &'a Allocator,
        mode: MapMode,
    ) -> Result<Self, CodeTransformError> {
        let len = check_source_len(source.len())?;
        if window.start > window.end {
            return Err(CodeTransformError::ReversedRange {
                start: window.start,
                end: window.end,
            });
        }
        if window.end > len {
            return Err(CodeTransformError::OutOfRange {
                offset: window.end,
                len,
            });
        }
        for offset in [window.start, window.end] {
            if !source.is_char_boundary(offset as usize) {
                return Err(CodeTransformError::MidChar { offset });
            }
        }
        let capacity = edit_hint.saturating_mul(2).saturating_add(1);
        let mut transform = Self::from_chunks(source, allocator, capacity, mode, window);
        // Only the window renders: the bytes outside it are not output.
        transform.output_delta = i64::from(window.end - window.start) - i64::from(len);
        Ok(transform)
    }

    /// Like [`new_in`](Self::new_in) but with an explicit chunk-buffer size. Style
    /// rewrites know their edit count and would otherwise pay the template-sized
    /// 64-slot floor on a one-insert rule.
    fn with_chunk_capacity_in(
        source: &'a str,
        allocator: &'a Allocator,
        estimated_chunks: usize,
        mode: MapMode,
    ) -> Self {
        // A source past `u32` offsets is refused by the checked window
        // constructor; the whole-source constructors serve sources a compiler
        // can hold.
        verter_debug_assert!(
            check_source_len(source.len()).is_ok(),
            "a source longer than u32 offsets needs CodeTransform::for_window_in"
        );
        let len = source.len() as u32;
        Self::from_chunks(source, allocator, estimated_chunks, mode, Span::new(0, len))
    }

    /// A transform whose one unedited chunk covers `window`.
    fn from_chunks(
        source: &'a str,
        allocator: &'a Allocator,
        estimated_chunks: usize,
        mode: MapMode,
        window: Span,
    ) -> Self {
        #[cfg(test)]
        CODE_TRANSFORM_CONSTRUCTIONS.with(|count| count.set(count.get() + 1));

        let mut chunks = Vec::with_capacity(estimated_chunks);
        if window.start < window.end {
            chunks.push(Chunk::from_source(window.start, window.end));
        }

        Self {
            original: source,
            chunks,
            scratch: Vec::with_capacity(estimated_chunks),
            intro: Vec::new(),
            intro_len: 0,
            outro: Vec::new(),
            outro_len: 0,
            allocator,
            cursor_hint: 0,
            output_delta: 0,
            resolver: OnceCell::new(),
            sourcemap_locations: Vec::new(),
            generated_unmapped_boundaries: FxHashMap::default(),
            unmapped_overwrite_contents: FxHashSet::default(),
            #[cfg(test)]
            last_reserved_token_capacity: std::cell::Cell::new(0),
            helper_preamble_content: None,
            helper_preamble_carrier_anchor: None,
            anchored_present: false,
            audit_ops: AuditOpTally::default(),
            map_mode: mode,
        }
    }

    /// The map policy chosen at construction.
    pub fn map_mode(&self) -> MapMode {
        self.map_mode
    }

    /// Test accessor: entries in the map-only side tables (explicit map
    /// locations, unmapped boundaries, synthetic overwrites, the helper
    /// preamble) and whether a position resolver was built.
    #[cfg(test)]
    pub(crate) fn captured_map_facts(&self) -> (usize, bool) {
        let entries = self.sourcemap_locations.len()
            + self.generated_unmapped_boundaries.len()
            + self.unmapped_overwrite_contents.len()
            + usize::from(self.helper_preamble_content.is_some());
        (entries, self.resolver.get().is_some())
    }

    /// Whether map-only facts are captured; every capture point asks first.
    #[inline]
    pub(super) fn captures_map(&self) -> bool {
        if self.map_mode == MapMode::Map {
            note_map_capture();
            true
        } else {
            false
        }
    }

    /// Get the original source text
    pub fn original(&self) -> &str {
        self.original
    }

    /// Refuse a map product of a transform that captured no map facts.
    pub(super) fn require_map(&self) -> Result<(), CodeTransformError> {
        match self.map_mode {
            MapMode::Map => Ok(()),
            MapMode::NoMap => Err(CodeTransformError::MapNotCaptured),
        }
    }

    /// Whether `content` is the replacement text of an
    /// [`overwrite_unmapped`](Self::overwrite_unmapped): wholly synthetic bytes
    /// with NO correspondence to the span they stand in for. Joined by pointer
    /// identity, so authored text that happens to equal generated bytes can
    /// never be mistaken for it.
    pub(super) fn is_unmapped_overwrite(&self, content: &str) -> bool {
        self.unmapped_overwrite_contents
            .contains(&(content.as_ptr() as usize, content.len()))
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
        if !self.captures_map() {
            return;
        }
        self.helper_preamble_content = Some(content);
    }

    /// Record the helper preamble and its authored zero-width insertion point.
    pub fn set_helper_preamble_content_at(&mut self, content: &'a str, carrier_anchor: u32) {
        if !self.captures_map() {
            return;
        }
        self.helper_preamble_content = Some(content);
        self.helper_preamble_carrier_anchor =
            (carrier_anchor <= self.original.len() as u32).then_some(carrier_anchor);
    }

    /// The recorded helper-import preamble insertion content, if any. Used by source-map
    /// generation to compute the typed preamble-end boundary.
    pub(super) fn helper_preamble_content(&self) -> Option<&'a str> {
        self.helper_preamble_content
    }

    pub(super) fn helper_preamble_carrier_anchor(&self) -> Option<u32> {
        self.helper_preamble_carrier_anchor
    }

    /// The unmapped transitions recorded inside generated `content`, as
    /// offsets relative to it.
    pub(super) fn unmapped_boundaries_in(&self, content: &str) -> &[u32] {
        self.generated_unmapped_boundaries
            .get(&(content.as_ptr() as usize, content.len()))
            .map_or(&[], |offsets| offsets.as_slice())
    }

    /// Allocate a string in the bump allocator, returning a reference with the
    /// same lifetime as the CodeTransform. Useful for deferring insertions.
    pub fn alloc_str(&self, s: &str) -> &'a str {
        self.allocator.alloc_str(s)
    }

    /// Single audit-count point for one `CodeTransform` operation. Called
    /// at the entry of every public op (prepend / append / overwrite /
    /// remove / move / batch) to populate
    /// [`verter_audit::payloads::compile::CompilePayload::code_transform_ops`].
    /// Optional capture: without `semantic-observe` this compiles to
    /// nothing; with it, it bumps a per-transform counter the transform
    /// publishes once, when dropped — an operation never reads the observer
    /// slot.
    #[inline(always)]
    pub(super) fn record_audit_op(&self) {
        self.audit_ops.bump();
    }

    /// Prepend content to the very start
    pub fn prepend(&mut self, content: &str) -> &mut Self {
        self.record_audit_op();
        if !content.is_empty() {
            self.intro.push(self.allocator.alloc_str(content));
            self.intro_len += content.len();
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
    /// required (debug-asserted). It records the STORED intro piece — NOT the `content` argument
    /// — because [`prepend`](Self::prepend) bump-allocates its own copy, which is the pointer
    /// source-map generation matches. The Svelte IDE projector uses this to keep its
    /// `@jsxImportSource`-led prelude as the leading output bytes while still publishing the typed
    /// preamble-end boundary.
    pub fn prepend_helper_preamble_content(&mut self, content: &str) -> &mut Self {
        // The boundary "end of intro" equals "end of helper preamble" ONLY when the intro IS exactly
        // the preamble — i.e. the intro was empty before this prepend. The debug-assert catches a
        // contract violation loudly in test/CI builds; the runtime check makes release builds FAIL
        // CLOSED (degrade to boundary-absent, which `generate_map_with_preamble`'s consumers already
        // handle) rather than record a preamble that is not the whole intro.
        let intro_was_empty = self.intro.is_empty();
        verter_debug_assert!(
            intro_was_empty,
            "prepend_helper_preamble_content requires an empty intro (single-prepend contract)"
        );
        self.prepend(content);
        let sole_piece = self.intro.last().copied().filter(|_| intro_was_empty);
        if let Some(piece) = sole_piece {
            // The intro now IS exactly `content` (a fresh bump alloc; old intro empty). Record that
            // STORED piece — NOT the `content` argument — because `generate_map_with_preamble`
            // matches the preamble by pointer identity.
            self.set_helper_preamble_content(piece);
        }
        self
    }

    /// Append content to the very end
    pub fn append(&mut self, content: &str) -> &mut Self {
        self.record_audit_op();
        if !content.is_empty() {
            self.outro.push(self.allocator.alloc_str(content));
            self.outro_len += content.len();
        }
        self
    }

    /// Prepend content before a specific position (inserts before the position).
    /// Multiple prepend_left calls at the same index stack in reverse order (last call first).
    pub fn prepend_left(&mut self, index: u32, content: &str) -> &mut Self {
        self.record_audit_op();
        if !content.is_empty() {
            let content_ref = self.allocator.alloc_str(content);
            self.prepend_left_allocated(index, content_ref);
        }
        self
    }

    fn prepend_left_allocated(&mut self, index: u32, content: &'a str) {
        let insert_idx = self.find_insert_position_for_prepend(index, false);
        self.chunks.insert(insert_idx, Chunk::inserted(content));
        if insert_idx <= self.cursor_hint {
            self.cursor_hint += 1;
        }
        self.output_delta += content.len() as i64;
    }

    pub(crate) fn prepend_left_with_generated_marker(
        &mut self,
        index: u32,
        content: &str,
        local_start: u32,
        local_end: u32,
    ) -> Option<GeneratedContentMarker<'a>> {
        if content.is_empty()
            || local_start > local_end
            || local_end as usize > content.len()
            || !content.is_char_boundary(local_start as usize)
            || !content.is_char_boundary(local_end as usize)
        {
            return None;
        }
        self.record_audit_op();
        let content = self.allocator.alloc_str(content);
        self.prepend_left_allocated(index, content);
        Some(GeneratedContentMarker {
            content,
            local_start,
            local_end,
        })
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
            Chunk::Original { start, .. }
            | Chunk::Overwritten { start, .. }
            | Chunk::OverwrittenSegmented { start, .. } => Some(*start),
            Chunk::Inserted { .. }
            | Chunk::Relocated { .. }
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
        // Every positioned chunk spans a non-empty source range, so none starts
        // at or past the end of the source: a position there is always past
        // every chunk. Output anchored at the end of the source is the dominant
        // append pattern and must not rescan the chunk list.
        if index as usize >= self.original.len() {
            return SplitResult::End;
        }
        for i in search_start..self.chunks.len() {
            count_chunk_visit();
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
                Chunk::Overwritten { start: s, .. }
                | Chunk::OverwrittenSegmented { start: s, .. } => {
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
                | Chunk::Relocated { .. }
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
                Chunk::Overwritten { start: s, .. }
                | Chunk::OverwrittenSegmented { start: s, .. } => {
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
                | Chunk::Relocated { .. }
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

    /// Overwrite a range with wholly synthetic replacement text that has no
    /// character-level correspondence to the span it replaces — so the
    /// resulting `Overwritten` chunk must emit NO source-map token, unlike
    /// [`overwrite`](Self::overwrite) (which claims the replaced span's
    /// start as the replacement's source position, truthful for genuine 1:1
    /// substitutions but false for e.g. a whole synthesized wrapper object
    /// literal replacing a `<script setup>` tag).
    ///
    /// Reuses the exact same splice engine as `overwrite` — every ordering
    /// interaction with other operations at the same position (prepends,
    /// moves) is IDENTICAL to a regular overwrite; only `generate_map`
    /// treats the resulting chunk as unmapped, via a content-identity
    /// side-table (the same pattern `generated_unmapped_boundaries` uses).
    pub fn overwrite_unmapped(&mut self, start: u32, end: u32, content: &str) -> &mut Self {
        self.record_audit_op();
        if start >= end {
            return self;
        }

        let content_ref = if content.is_empty() {
            ""
        } else {
            self.allocator.alloc_str(content)
        };
        if !content_ref.is_empty() && self.captures_map() {
            self.unmapped_overwrite_contents
                .insert((content_ref.as_ptr() as usize, content_ref.len()));
        }

        self.replace_range_impl(start, end, content_ref, false);
        self
    }

    /// Same as [`overwrite_unmapped`](Self::overwrite_unmapped), but
    /// `content` is already bump-allocated against THIS transform's own
    /// allocator (`content: &'a str`, not a re-borrowed `&str`) — skips the
    /// re-allocation `overwrite_unmapped` always performs, so the caller's
    /// original pointer survives into the resulting chunk unchanged. A
    /// caller that later mints a
    /// [`mark_generated_sub_range`](Self::mark_generated_sub_range) fact
    /// against that same pointer (e.g.
    /// `CodeGenOutput::record_sfc_export_fact`) requires this variant:
    /// `overwrite_unmapped`'s unconditional re-allocation would mint a
    /// pointer the fact could never match.
    pub(crate) fn overwrite_unmapped_alloc(
        &mut self,
        start: u32,
        end: u32,
        content: &'a str,
    ) -> &mut Self {
        self.record_audit_op();
        if start >= end {
            return self;
        }

        if !content.is_empty() && self.captures_map() {
            self.unmapped_overwrite_contents
                .insert((content.as_ptr() as usize, content.len()));
        }

        self.replace_range_impl(start, end, content, false);
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
        if boundaries.is_empty() || !self.captures_map() {
            return;
        }
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
                }
                | Chunk::OverwrittenSegmented {
                    start: chunk_start,
                    end: chunk_end,
                    content,
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
                // Relocated runs and pure-insertion chunks don't participate in overwrite resolution
                Chunk::Relocated { .. }
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
                    Chunk::Relocated { .. }
                    | Chunk::Inserted { .. }
                    | Chunk::InsertedMapped { .. }
                    | Chunk::InsertedAnchored { .. } => {
                        removed_output += i64::from(chunk.output_len());
                    }
                    Chunk::Original { .. }
                    | Chunk::Overwritten { .. }
                    | Chunk::OverwrittenSegmented { .. } => {}
                }
            }
            self.chunks.splice(first..=last, replacement_chunks);
            self.cursor_hint = first;
        } else if !handled {
            verter_debug_assert!(
                false,
                "replacement of [{start}, {end}) found no live chunk: the range was relocated \
                 (use try_overwrite for a typed refusal)"
            );
            self.chunks
                .push(Chunk::overwritten(start, end, content_ref));
            self.cursor_hint = self.chunks.len() - 1;
        } else {
            verter_debug_assert!(
                false,
                "replacement of [{start}, {end}) found no live chunk: the range was relocated \
                 (use try_overwrite for a typed refusal)"
            );
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

    pub(crate) fn move_with_suffix_and_generated_marker(
        &mut self,
        start: u32,
        end: u32,
        target_index: u32,
        suffix: &str,
        local_start: u32,
        local_end: u32,
    ) -> Option<GeneratedContentMarker<'a>> {
        if suffix.is_empty()
            || local_start > local_end
            || local_end as usize > suffix.len()
            || !suffix.is_char_boundary(local_start as usize)
            || !suffix.is_char_boundary(local_end as usize)
        {
            return None;
        }
        let suffix = self.allocator.alloc_str(suffix);
        let marker = GeneratedContentMarker {
            content: suffix,
            local_start,
            local_end,
        };
        self.move_wrapped_allocated(start, end, target_index, None, Some(suffix));
        Some(marker)
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
        let prefix = (!prefix.is_empty()).then(|| self.allocator.alloc_str(prefix));
        let suffix = (!suffix.is_empty()).then(|| self.allocator.alloc_str(suffix));
        self.move_wrapped_allocated(start, end, target_index, prefix, suffix)
    }

    pub(super) fn move_wrapped_allocated(
        &mut self,
        start: u32,
        end: u32,
        target_index: u32,
        prefix: Option<&'a str>,
        suffix: Option<&'a str>,
    ) -> &mut Self {
        self.record_audit_op();
        if start >= end {
            return self;
        }

        // Single-pass: split at boundaries + collect indices in one forward scan
        // (replaces ensure_split_at(start) + ensure_split_at(end) + identification loop)
        let mut indices_to_move: SmallVec<[usize; 8]> = SmallVec::new();
        // Position watermark: tracks the end of the last positioned chunk we've
        // seen. Used to decide whether unpositioned chunks (insertions, relocated runs) fall
        // within the [start, end) move range — they belong to the range if the
        // watermark has entered it.
        let mut current_pos = 0u32;
        // Every chunk before a positioned chunk that starts strictly before
        // `start` keeps the watermark below `start` (positions are monotonic
        // and ranges disjoint), so none of them is in the range: the scan may
        // resume from such a chunk instead of the beginning.
        let mut i = self.move_search_start(start);
        let mut past_start = false;

        while i < self.chunks.len() {
            count_chunk_visit();
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
                }
                | Chunk::OverwrittenSegmented {
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
                Chunk::Relocated { .. }
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
        // Moves are net-zero for existing content, but prefix/suffix are new insertions
        self.output_delta += prefix.map_or(0, str::len) as i64 + suffix.map_or(0, str::len) as i64;

        // The run keeps the chunks the range holds NOW, by reference: what a
        // move carries is fixed when the move is called, and every member
        // keeps its own correspondence. No text is copied.
        let members: SmallVec<[Chunk<'a>; 8]> =
            indices_to_move.iter().map(|&i| self.chunks[i]).collect();
        let members: &'a [Chunk<'a>] = self.allocator.alloc_slice_copy(members.as_slice());
        let mut chunks_to_move: SmallVec<[Chunk<'a>; 3]> = SmallVec::new();
        if let Some(prefix) = prefix {
            chunks_to_move.push(Chunk::inserted(prefix));
        }
        chunks_to_move.push(Chunk::Relocated { members });
        if let Some(suffix) = suffix {
            chunks_to_move.push(Chunk::inserted(suffix));
        }

        // Remove the moved chunks. Chunks before the first one stay in place,
        // and a contiguous run (the common case) is removed in one shift.
        let first = indices_to_move[0];
        let last = indices_to_move[indices_to_move.len() - 1];
        if last - first + 1 == indices_to_move.len() {
            self.chunks.drain(first..=last);
        } else {
            let mut write = first;
            let mut remove_idx = 0usize;
            for read in first..self.chunks.len() {
                if remove_idx < indices_to_move.len() && indices_to_move[remove_idx] == read {
                    remove_idx += 1;
                } else {
                    self.chunks[write] = self.chunks[read];
                    write += 1;
                }
            }
            self.chunks.truncate(write);
        }
        // The chunks around the removed range are where a forward-progressing
        // sequence of moves resumes.
        self.cursor_hint = first;

        // Insert moved chunks at target position
        let insert_idx = self.find_insert_position_for_append(target_index, false);
        let insert_pos = insert_idx.min(self.chunks.len());
        self.chunks.splice(insert_pos..insert_pos, chunks_to_move);

        self
    }

    /// Emit the AUTHORED bytes of `[start, end)` at `target_index`, wrapped
    /// in unmapped `prefix` and `suffix`, whatever other edits do to the
    /// range: the extracted run reads the source, not the chunk list, and the
    /// range itself is left in place. The run maps to the source like any
    /// relocated authored text.
    pub fn extract_original(
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
        let members: &'a [Chunk<'a>] = self
            .allocator
            .alloc_slice_copy(&[Chunk::from_source(start, end)]);
        let mut run: SmallVec<[Chunk<'a>; 3]> = SmallVec::new();
        if !prefix.is_empty() {
            run.push(Chunk::inserted(self.allocator.alloc_str(prefix)));
        }
        run.push(Chunk::Relocated { members });
        if !suffix.is_empty() {
            run.push(Chunk::inserted(self.allocator.alloc_str(suffix)));
        }
        self.output_delta += i64::from(end - start) + prefix.len() as i64 + suffix.len() as i64;
        let insert_idx = self.find_insert_position_for_append(target_index, false);
        let insert_pos = insert_idx.min(self.chunks.len());
        self.chunks.splice(insert_pos..insert_pos, run);
        self
    }

    /// Where a move of a range starting at `start` may begin its scan: the
    /// cursor hint, or the chunk before it, when that chunk is positioned
    /// strictly before `start`; otherwise the beginning.
    fn move_search_start(&self, start: u32) -> usize {
        let hint = self.cursor_hint.min(self.chunks.len().saturating_sub(1));
        if self.chunks.is_empty() {
            return 0;
        }
        for candidate in [hint, hint.saturating_sub(1)] {
            if Self::chunk_position(&self.chunks[candidate]).is_some_and(|pos| pos < start) {
                return candidate;
            }
        }
        0
    }

    /// Get a slice of the original source
    pub fn slice(&self, start: u32, end: u32) -> &str {
        &self.original[start as usize..end as usize]
    }

    /// Check if the content has been modified
    pub fn is_modified(&self) -> bool {
        self.intro_len != 0 || self.outro_len != 0 || self.chunks.iter().any(|c| !c.is_original())
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

    /// The intro pieces, in output order.
    pub(super) fn intro(&self) -> impl Iterator<Item = &'a str> + '_ {
        self.intro.iter().rev().copied()
    }

    /// Total bytes of the intro.
    pub(super) fn intro_len(&self) -> usize {
        self.intro_len
    }

    /// The intro when it is exactly one piece — the only shape in which the
    /// intro can be the recorded helper preamble.
    pub(super) fn sole_intro_piece(&self) -> Option<&'a str> {
        match self.intro.as_slice() {
            [piece] => Some(piece),
            _ => None,
        }
    }

    /// The outro pieces, in output order.
    pub(super) fn outro(&self) -> impl Iterator<Item = &'a str> + '_ {
        self.outro.iter().copied()
    }

    /// Total bytes of the outro.
    pub(super) fn outro_len(&self) -> usize {
        self.outro_len
    }

    // Batch chunk-rebuild operations (`batch_prepend_left_static`,
    // `batch_prepend_left_with_source_map`, `batch_prepend_left_merged`,
    // `batch_overwrite`) live in the sibling `batch_ops` module.
}

impl<'a, M: verter_audit::observe::ObserveExec> CodeTransform<'a, M> {
    /// Build the final output string with pre-allocated capacity.
    #[must_use]
    #[cfg_attr(feature = "semantic-observe", hotpath::measure)]
    pub fn build_string(&self) -> String {
        #[cfg(test)]
        CODE_TRANSFORM_BUILD_STRING_CALLS.with(|count| count.set(count.get() + 1));

        // Capacity from tracked delta — avoids a full scan of all chunks.
        let capacity = (self.original.len() as i64 + self.output_delta) as usize
            + self.intro_len
            + self.outro_len;

        verter_audit::attribute_n!(CodeTransformRender, capacity);
        let mut out = String::with_capacity(capacity);
        self.write_output_to(&mut out)
            .expect("writing to a String cannot fail");
        out
    }

    /// The text of one leaf chunk (never a relocated run).
    fn leaf_text<'s>(&'s self, chunk: &'s Chunk<'a>) -> &'s str {
        match chunk {
            Chunk::Original { start, end } => &self.original[*start as usize..*end as usize],
            Chunk::Inserted { content }
            | Chunk::Overwritten { content, .. }
            | Chunk::OverwrittenSegmented { content, .. }
            | Chunk::InsertedMapped { content, .. }
            | Chunk::InsertedAnchored { content, .. } => content,
            Chunk::Relocated { .. } => {
                unreachable!("the leaf walk expands relocated runs into their members")
            }
        }
    }

    /// Every chunk in output order with relocated runs expanded, each with
    /// whether it was relocated.
    pub(super) fn leaf_chunks(&self) -> LeafChunks<'_, 'a> {
        LeafChunks::new(&self.chunks)
    }

    /// Build the output and retain the source-bearing range geometry.
    ///
    /// This is deliberately not a second source-map implementation. It exposes
    /// the same chunk authority used by [`Self::generate_map`] so a compound
    /// emitter can relocate transformed fragments and lower the final artifact
    /// through one ordinary `CodeTransform` map. Synthesized insertions are not
    /// represented; callers therefore cannot accidentally assign them authored
    /// provenance.
    ///
    /// # Errors
    ///
    /// [`CodeTransformError::MapNotCaptured`] for a transform created without
    /// map capture.
    pub fn build_string_with_source_ranges(
        &self,
    ) -> Result<(String, Vec<GeneratedSourceRange>), CodeTransformError> {
        let (output, ranges, _) = self.build_string_with_source_ranges_and_unmapped_boundaries()?;
        Ok((output, ranges))
    }

    /// Resolve a registered generated-content marker into built-output bytes.
    /// Pointer identity, not content equality, joins the marker to its chunk.
    pub(crate) fn generated_content_range(
        &self,
        marker: GeneratedContentMarker<'a>,
    ) -> Option<std::ops::Range<u32>> {
        let mut generated = self.intro_len as u32;
        let mut found = None;
        for (chunk, _) in self.leaf_chunks() {
            let content = match chunk {
                Chunk::Original { start, end } => {
                    generated += end - start;
                    continue;
                }
                Chunk::Inserted { content }
                | Chunk::Overwritten { content, .. }
                | Chunk::OverwrittenSegmented { content, .. }
                | Chunk::InsertedMapped { content, .. }
                | Chunk::InsertedAnchored { content, .. } => *content,
                Chunk::Relocated { .. } => unreachable!("leaf walk"),
            };
            if std::ptr::eq(content.as_ptr(), marker.content.as_ptr())
                && content.len() == marker.content.len()
            {
                let range = generated + marker.local_start..generated + marker.local_end;
                if found.replace(range).is_some() {
                    return None;
                }
            }
            generated += content.len() as u32;
        }
        found
    }

    /// Mint an additional marker over a local sub-range of `content` a
    /// PRIOR call already inserted using the exact same bump-allocated
    /// pointer (e.g. [`overwrite_unmapped`](Self::overwrite_unmapped)'s
    /// `content_ref`, or any other already-inserted chunk content the
    /// caller retained). Mints only — never inserts anything — so one
    /// already-inserted chunk can carry several independently-resolvable
    /// declared positions (e.g. every `__sfc__` binding reference plus the
    /// terminal default-export statement a script producer wrote in one
    /// generated block) without re-allocating or re-inserting the content
    /// once per position.
    pub(crate) fn mark_generated_sub_range(
        &self,
        content: &'a str,
        local_start: u32,
        local_end: u32,
    ) -> Option<GeneratedContentMarker<'a>> {
        if local_start > local_end
            || local_end as usize > content.len()
            || !content.is_char_boundary(local_start as usize)
            || !content.is_char_boundary(local_end as usize)
        {
            return None;
        }
        Some(GeneratedContentMarker {
            content,
            local_start,
            local_end,
        })
    }

    /// Build the output, authored range geometry, and explicit ownership
    /// transitions inside generated chunks.
    #[allow(clippy::type_complexity)]
    pub(crate) fn build_string_with_source_ranges_and_unmapped_boundaries(
        &self,
    ) -> Result<(String, Vec<GeneratedSourceRange>, Vec<u32>), CodeTransformError> {
        self.require_map()?;
        Ok(self.captured_source_ranges())
    }

    /// Output, authored range geometry and unmapped transitions of a
    /// transform that captured map facts.
    pub(super) fn captured_source_ranges(&self) -> (String, Vec<GeneratedSourceRange>, Vec<u32>) {
        // Text and ranges in one pass over the pieces each chunk becomes.
        let capacity = (self.original.len() as i64 + self.output_delta) as usize
            + self.intro_len
            + self.outro_len;
        let mut output = String::with_capacity(capacity);
        for piece in self.intro() {
            output.push_str(piece);
        }
        let mut ranges = Vec::with_capacity(self.chunks.len());
        let mut unmapped_boundaries = Vec::new();
        let mut locations = self.sourcemap_locations.clone();
        locations.sort_unstable();
        locations.dedup();
        let mut pieces: SmallVec<[Piece<'a>; 4]> = SmallVec::new();
        for (chunk, relocated) in self.leaf_chunks() {
            count_chunk_visit();
            if let Some(content) = chunk_content(chunk) {
                let generated = output.len() as u32;
                unmapped_boundaries.extend(
                    self.unmapped_boundaries_in(content)
                        .iter()
                        .map(|offset| generated + offset),
                );
            }
            pieces.clear();
            self.leaf_pieces(chunk, relocated, &mut |piece| pieces.push(piece));
            for piece in &pieces {
                let text = self.piece_text(piece);
                let generated = output.len() as u32;
                let len = text.len() as u32;
                if len > 0 {
                    match piece.provenance() {
                        Provenance::Exact { source_start } => push_preserved_source_ranges(
                            &mut ranges,
                            &locations,
                            generated,
                            source_start,
                            len,
                        ),
                        Provenance::Rewritten { source } => ranges.push(GeneratedSourceRange {
                            generated_start: generated,
                            generated_end: generated + len,
                            source_start: source.start,
                            source_end: source.end,
                            replacement: true,
                        }),
                        // A point accounts for no authored extent.
                        Provenance::Anchor { source_offset } => {
                            ranges.push(GeneratedSourceRange {
                                generated_start: generated,
                                generated_end: generated + len,
                                source_start: source_offset,
                                source_end: source_offset,
                                replacement: true,
                            });
                        }
                        // Synthetic text never reaches a range consumer.
                        Provenance::Unmapped | Provenance::External(_) => {}
                    }
                }
                output.push_str(text);
            }
        }
        for piece in self.outro() {
            output.push_str(piece);
        }
        unmapped_boundaries.sort_unstable();
        unmapped_boundaries.dedup();
        (output, ranges, unmapped_boundaries)
    }
}

impl<'a, M: verter_audit::observe::ObserveExec> CodeTransform<'a, M> {
    /// Write the full output to any `fmt::Write` sink.
    fn write_output_to(&self, w: &mut impl std::fmt::Write) -> std::fmt::Result {
        for piece in self.intro() {
            w.write_str(piece)?;
        }
        for (chunk, _) in self.leaf_chunks() {
            w.write_str(self.leaf_text(chunk))?;
        }
        for piece in self.outro() {
            w.write_str(piece)?;
        }
        Ok(())
    }
}

impl<'a, M: verter_audit::observe::ObserveExec> std::fmt::Display for CodeTransform<'a, M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.write_output_to(f)
    }
}
