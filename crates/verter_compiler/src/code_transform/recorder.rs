//! Deferred source edits, recorded while a producer walks its input and
//! applied to a [`CodeTransform`] in one pass.
//!
//! The recorder is part of the transform's owner: it states what to edit and
//! the transform resolves every edit. [`EditRecorder::apply`] applies the
//! recorded edits in a fixed order that does not depend on the order they
//! were recorded in — replacements, then extractions of authored text, then
//! insertions, then relocations — and resolves replacements by one law:
//! sorted by start ascending, end descending and record order, a replacement
//! inside a wider one (in any channel) is dropped, of two equal ranges the
//! first recorded wins, and any other overlap is a
//! [`CodeTransformError::Conflict`].

use oxc_allocator::Allocator;
use smallvec::SmallVec;
use verter_span::Span;

use super::code_transform::CodeTransform;
use super::fallible::CodeTransformError;
use super::segmented::SegmentAnchor;
use crate::emission::{note_map_capture, MapMode};

/// A position in the ordered prepend channel whose text is supplied later
/// (see [`EditRecorder::reserve_ordered_unmapped`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReservedPrepend(usize);

/// How a recorded replacement corresponds to the range it replaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReplacementKind<'alloc> {
    /// The replacement stands for the range: one map token at its start.
    Mapped,
    /// Wholly synthetic text with no correspondence to the range.
    Unmapped,
    /// Synthetic text embedding authored lexemes, each a checked claim.
    Segmented(&'alloc [SegmentAnchor]),
}

/// One recorded replacement of `[start, end)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Replacement<'alloc> {
    pub(crate) start: u32,
    pub(crate) end: u32,
    pub(crate) content: &'alloc str,
    pub(crate) kind: ReplacementKind<'alloc>,
}

/// What a recorded relocation carries to its target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RelocationIntent {
    /// The range's EDITED content moves; the range is left empty.
    Relocate,
    /// The range's AUTHORED bytes are emitted at the target; the range and
    /// its edits stay where they are.
    ExtractOriginal,
}

#[derive(Debug, Clone, Copy)]
struct Relocation<'alloc> {
    start: u32,
    end: u32,
    target: u32,
    prefix: &'alloc str,
    suffix: &'alloc str,
    intent: RelocationIntent,
}

/// Deferred source edits for one transform.
///
/// Nothing is applied until [`apply`](Self::apply). Content is bump-allocated
/// in the allocator the transform will use, so applying copies no text.
#[derive(Clone)]
pub struct EditRecorder<'alloc> {
    /// Every replacement, in record order, with its correspondence kind.
    replacements: Vec<Replacement<'alloc>>,

    /// Insert content before a position: (position, content).
    /// Used for binding prefixes (`_ctx.`, `$setup.`), suffixes (`.value`), separators.
    pub(crate) prepends: Vec<(u32, &'alloc str)>,

    /// Insert content before a position with source map mapping:
    /// (insertion_pos, source_pos, content_offset, content).
    /// Creates `InsertedMapped` chunks that emit source map tokens at `source_pos`.
    /// `content_offset` shifts the token within the content (characters before it
    /// are unmapped). Used for relocated directive expressions (v-if conditions,
    /// v-for iterables) where binding prefixes precede the original identifier.
    pub(crate) mapped_prepends: Vec<(u32, u32, u32, &'alloc str)>,

    /// Relocations and extractions, in record order.
    relocations: Vec<Relocation<'alloc>>,

    /// Explicit source-map boundary offsets, transferred in
    /// [`apply`](Self::apply) via `try_add_sourcemap_location`.
    /// `Original` chunks otherwise token only start and newlines; a
    /// registered offset gets a mid-line token (e.g. a top-level identifier
    /// the `verbatim-carry` oracle must land on).
    sourcemap_locations: Vec<u32>,

    /// Allocator reference for bump-allocating generated strings.
    alloc: &'alloc Allocator,

    /// Whether map-only facts are recorded, chosen at construction. It must
    /// equal the policy of the transform the edits are applied to.
    map_mode: MapMode,

    /// The leading helper-import preamble insertion, when one was recorded
    /// with [`prepend_helper_preamble`](Self::prepend_helper_preamble): the
    /// carrier anchor it stands for and its bump-allocated content. Handed to
    /// the transform in [`apply`](Self::apply) so source-map generation can
    /// report the generated position immediately after it.
    helper_preamble: Option<(Option<u32>, &'alloc str)>,

    /// One reusable scratch buffer for the `write!`-style format sinks
    /// (`overwrite_fmt`, `prepend_fmt`, and the mapped variants). Each sink
    /// clears and reuses it, so a formatted emission costs the retained heap
    /// capacity plus one bump copy — never a fresh `String` per call. It is
    /// an operation-construction helper only; it never holds built output.
    scratch: String,
}

impl<'alloc> EditRecorder<'alloc> {
    /// Create an empty recorder that records map facts.
    pub fn new(alloc: &'alloc Allocator) -> Self {
        Self::with_mode(alloc, MapMode::Map)
    }

    /// Create an empty recorder with the map policy chosen here, before any
    /// producer records. Under [`MapMode::NoMap`] no map-only fact is
    /// recorded: mapped insertions are stored as synthetic text, segmented
    /// anchors and explicit map locations are dropped.
    pub fn with_mode(alloc: &'alloc Allocator, map_mode: MapMode) -> Self {
        Self {
            replacements: Vec::with_capacity(16),
            prepends: Vec::with_capacity(16),
            mapped_prepends: Vec::new(),
            relocations: Vec::new(),
            sourcemap_locations: Vec::new(),
            alloc,
            map_mode,
            helper_preamble: None,
            scratch: String::new(),
        }
    }

    /// Whether map-only facts are recorded; every capture point asks first.
    #[inline]
    fn captures_map(&self) -> bool {
        if self.map_mode == MapMode::Map {
            note_map_capture();
            true
        } else {
            false
        }
    }

    /// A buffer for segmented-overwrite anchors that records nothing under
    /// [`MapMode::NoMap`].
    pub fn anchors(&self) -> AnchorBuf {
        AnchorBuf {
            anchors: SmallVec::new(),
            map_mode: self.map_mode,
        }
    }

    /// Record an ordered insertion whose text from `content_offset` on
    /// stands for `source_pos`; synthetic text under [`MapMode::NoMap`].
    #[inline]
    fn push_mapped(
        &mut self,
        pos: u32,
        source_pos: u32,
        content_offset: u32,
        content: &'alloc str,
    ) {
        if self.captures_map() {
            self.mapped_prepends
                .push((pos, source_pos, content_offset, content));
        } else {
            self.mapped_prepends
                .push((pos, 0, content.len() as u32, content));
        }
    }

    #[inline]
    fn push_replacement(
        &mut self,
        start: u32,
        end: u32,
        content: &'alloc str,
        kind: ReplacementKind<'alloc>,
    ) {
        self.replacements.push(Replacement {
            start,
            end,
            content,
            kind,
        });
    }

    /// The recorded replacements of one kind, in record order.
    #[cfg(test)]
    pub(crate) fn replacements_of(
        &self,
        kind: fn(&ReplacementKind<'alloc>) -> bool,
    ) -> Vec<(u32, u32, &'alloc str)> {
        self.replacements
            .iter()
            .filter(|replacement| kind(&replacement.kind))
            .map(|replacement| (replacement.start, replacement.end, replacement.content))
            .collect()
    }

    /// Push an overwrite operation. The content is bump-allocated.
    #[inline]
    pub fn overwrite(&mut self, start: u32, end: u32, content: &str) {
        let allocated = self.alloc.alloc_str(content);
        self.push_replacement(start, end, allocated, ReplacementKind::Mapped);
    }

    /// Push an overwrite with pre-allocated content (avoids double allocation).
    #[inline]
    pub fn overwrite_alloc(&mut self, start: u32, end: u32, content: &'alloc str) {
        self.push_replacement(start, end, content, ReplacementKind::Mapped);
    }

    /// Push an unmapped overwrite: wholly synthetic content with no
    /// character-level correspondence to the replaced span. The content is
    /// bump-allocated.
    #[inline]
    pub fn overwrite_unmapped(&mut self, start: u32, end: u32, content: &str) {
        let allocated = self.alloc.alloc_str(content);
        self.push_replacement(start, end, allocated, ReplacementKind::Unmapped);
    }

    /// Same as [`overwrite_unmapped`](Self::overwrite_unmapped), but
    /// `content` is already bump-allocated — avoids a second allocation,
    /// and lets the caller later mint a
    /// [`CodeTransform::mark_generated_sub_range`] fact against the EXACT
    /// pointer this call inserts.
    #[inline]
    pub fn overwrite_unmapped_alloc(&mut self, start: u32, end: u32, content: &'alloc str) {
        self.push_replacement(start, end, content, ReplacementKind::Unmapped);
    }

    /// Segmented overwrite: `content` replaces `[start, end)`; each `anchors`
    /// entry claims a correspondence between bytes of `content` and the
    /// source (see [`SegmentAnchor`]). The transform checks every claim when
    /// the edit is applied.
    #[inline]
    pub fn overwrite_segmented(
        &mut self,
        start: u32,
        end: u32,
        content: &str,
        anchors: &[SegmentAnchor],
    ) {
        let content_ref = self.alloc.alloc_str(content);
        let anchors_ref = if anchors.is_empty() || !self.captures_map() {
            &[]
        } else {
            self.alloc_segment_anchors(anchors)
        };
        self.push_replacement(
            start,
            end,
            content_ref,
            ReplacementKind::Segmented(anchors_ref),
        );
    }

    /// Push a prepend-left (insert before position) with `&'static str` content.
    /// Zero allocation — the string lives in the binary.
    #[inline]
    pub fn prepend_static(&mut self, pos: u32, content: &'static str) {
        self.prepends.push((pos, content));
    }

    /// Push a prepend-left with bump-allocated content.
    #[inline]
    pub fn prepend_alloc(&mut self, pos: u32, content: &str) {
        let allocated = self.alloc.alloc_str(content);
        self.prepends.push((pos, allocated));
    }

    /// Push a prepend-left whose content is already bump-allocated.
    #[inline]
    pub fn prepend_allocated(&mut self, pos: u32, content: &'alloc str) {
        self.prepends.push((pos, content));
    }

    /// Root wrapper insertion for a raw template unit whose synthetic root has
    /// a zero-width opening tag. It must precede child operations at the same
    /// byte boundary.
    ///
    /// Nonzero-width (a real tag, e.g. `<script setup>`, replaced by synthetic
    /// wrapper) uses an unmapped overwrite: still a replacement (ordering vs
    /// later relocations stays intact) but no source-map token. A mapped
    /// replacement would claim the tag's start for the entire replacement —
    /// truthful only for 1:1 substitutions. Official rc.5 `prependLeft` wrapper
    /// insertion never touches the tag and carries no mapping either.
    pub fn overwrite_or_root_prefix(&mut self, start: u32, end: u32, content: &str) {
        if start == end {
            let allocated = self.alloc.alloc_str(content);
            self.prepends.insert(0, (start, allocated));
        } else {
            self.overwrite_unmapped(start, end, content);
        }
    }

    /// Same dispatch as
    /// [`overwrite_or_root_prefix`](Self::overwrite_or_root_prefix), but
    /// `content` is already bump-allocated.
    pub fn overwrite_or_root_prefix_alloc(&mut self, start: u32, end: u32, content: &'alloc str) {
        if start == end {
            self.prepends.insert(0, (start, content));
        } else {
            self.overwrite_unmapped_alloc(start, end, content);
        }
    }

    /// Segmented [`overwrite_or_root_prefix`](Self::overwrite_or_root_prefix):
    /// authored anchors (e.g. a hoisted static-props `class` key) survive.
    /// Zero-width stays an unmapped prepend; nonzero-width goes through
    /// [`overwrite_segmented`](Self::overwrite_segmented).
    pub fn overwrite_or_root_prefix_segmented(
        &mut self,
        start: u32,
        end: u32,
        content: &str,
        anchors: &[SegmentAnchor],
    ) {
        if start == end {
            let allocated = self.alloc.alloc_str(content);
            self.prepends.insert(0, (start, allocated));
        } else {
            self.overwrite_segmented(start, end, content, anchors);
        }
    }

    /// Root wrapper close for a raw template unit whose synthetic closing tag
    /// is a zero-width boundary after all authored bytes.
    ///
    /// See [`overwrite_or_root_prefix`](Self::overwrite_or_root_prefix) for
    /// why the nonzero-width case uses an unmapped overwrite rather than a
    /// regular (falsely position-claiming) one.
    pub fn overwrite_or_root_suffix(&mut self, start: u32, end: u32, content: &str) {
        if start == end {
            self.prepend_alloc(start, content);
        } else {
            self.overwrite_unmapped(start, end, content);
        }
    }

    /// Same dispatch as
    /// [`overwrite_or_root_suffix`](Self::overwrite_or_root_suffix), but
    /// `content` is already bump-allocated.
    pub fn overwrite_or_root_suffix_alloc(&mut self, start: u32, end: u32, content: &'alloc str) {
        if start == end {
            self.prepends.push((start, content));
        } else {
            self.overwrite_unmapped_alloc(start, end, content);
        }
    }

    /// Register an explicit source-map boundary at an original-source byte
    /// offset (see `sourcemap_locations` field doc). A no-op for source-map
    /// purposes when the position ends up outside any surviving authored
    /// chunk or mid-codepoint — `CodeTransform::try_add_sourcemap_location`
    /// degrades gracefully rather than corrupting the map.
    #[inline]
    pub fn add_sourcemap_location(&mut self, offset: u32) {
        if self.captures_map() {
            self.sourcemap_locations.push(offset);
        }
    }

    /// Push a source-mapped prepend-left with bump-allocated content.
    /// The generated chunk maps back to `source_pos` in the source map.
    /// The source map token is placed at the start of the content (offset 0).
    #[inline]
    pub fn prepend_alloc_mapped(&mut self, pos: u32, source_pos: u32, content: &str) {
        let allocated = self.alloc.alloc_str(content);
        self.push_mapped(pos, source_pos, 0, allocated);
    }

    /// Push an UNMAPPED prepend-left that interleaves with mapped prepends at the
    /// same position in insertion order.
    ///
    /// Plain [`prepend_static`](Self::prepend_static) / [`prepend_alloc`](Self::prepend_alloc)
    /// land in the `prepends` vec, which `apply` places BEFORE all
    /// `mapped_prepends` at the same position — so an unmapped→mapped→unmapped
    /// sequence at one anchor (e.g. `value={` + mapped `count` + `}` for relocated
    /// `v-model`) would reorder to `value={}` + `count`. Routing the unmapped text
    /// through `mapped_prepends` with `content_offset == content.len()` keeps it in
    /// the same vec (token sits one-past-the-end → emits no mapping) so the order
    /// is preserved. This mirrors the v-for `.map((` bridge emission.
    #[inline]
    pub fn prepend_ordered_unmapped(&mut self, pos: u32, content: &str) {
        if content.is_empty() {
            return;
        }
        let allocated = self.alloc.alloc_str(content);
        let len = allocated.len() as u32;
        self.mapped_prepends.push((pos, 0, len, allocated));
    }

    /// Reserve an UNMAPPED ordered prepend at `pos` whose text is decided later
    /// with [`fill_reserved`](Self::fill_reserved).
    ///
    /// The reservation takes its place in the ordered (`mapped_prepends`)
    /// channel NOW, so the eventual text lands after every ordered prepend
    /// already recorded at `pos` and before every one recorded later — even
    /// though it is only known once the content after it has been walked. A
    /// reservation that is never filled emits nothing.
    #[inline]
    pub fn reserve_ordered_unmapped(&mut self, pos: u32) -> ReservedPrepend {
        let index = self.mapped_prepends.len();
        self.mapped_prepends.push((pos, 0, 0, ""));
        ReservedPrepend(index)
    }

    /// Give a reservation its text (synthetic, unmapped).
    #[inline]
    pub fn fill_reserved(&mut self, reserved: ReservedPrepend, content: &str) {
        let allocated = self.alloc.alloc_str(content);
        let entry = &mut self.mapped_prepends[reserved.0];
        entry.2 = allocated.len() as u32;
        entry.3 = allocated;
    }

    /// Push a source-mapped prepend-left with a content offset.
    /// Characters before `content_offset` are unmapped; the source map token is
    /// placed at `content_offset`, pointing to `source_pos`. Used when binding
    /// prefixes (e.g., `(__props.`) precede the original identifier.
    #[inline]
    pub fn prepend_alloc_mapped_with_offset(
        &mut self,
        pos: u32,
        source_pos: u32,
        content_offset: u32,
        content: &str,
    ) {
        let allocated = self.alloc.alloc_str(content);
        self.push_mapped(pos, source_pos, content_offset, allocated);
    }

    /// Write `args` into the reusable scratch buffer and bump-allocate the
    /// result once. The buffer is cleared first and its capacity is retained
    /// for the next emission, so repeated formatted emissions reuse one
    /// allocation instead of allocating a fresh `String` per call.
    ///
    /// This is the arena-string companion to the op-recording format sinks
    /// (`overwrite_fmt` / `prepend_fmt` / …): those record an edit, whereas
    /// this returns the bump-allocated `&'alloc str` for content that is
    /// stored and assembled later rather than applied at a source position —
    /// e.g. the Vapor navigation (`const pN = _child(nM)`) and text-creation
    /// (`const xN = _txt(pP)`) lines accumulated for the render-function body.
    /// It produces a string through the SAME reusable scratch + single-bump
    /// path, never by editing already-emitted output in place.
    #[inline]
    pub(crate) fn alloc_fmt(&mut self, args: std::fmt::Arguments<'_>) -> &'alloc str {
        use std::fmt::Write as _;
        self.scratch.clear();
        // Appending to a `String` never fails; `write_fmt` can only return
        // `Err` if a `Display`/`Debug` impl inside `args` does. Surface that
        // as a panic rather than silently recording partial content.
        self.scratch
            .write_fmt(args)
            .expect("edit recorder format sink: Display impl must not fail");
        self.alloc.alloc_str(&self.scratch)
    }

    /// Push an overwrite whose content is produced by a `write!`-style format
    /// directly into the reusable scratch buffer, then bump-allocated once.
    ///
    /// Output-equivalent to `overwrite(start, end, &format!(...))` but avoids
    /// the intermediate per-call `String` allocation.
    #[inline]
    pub fn overwrite_fmt(&mut self, start: u32, end: u32, args: std::fmt::Arguments<'_>) {
        let allocated = self.alloc_fmt(args);
        self.push_replacement(start, end, allocated, ReplacementKind::Mapped);
    }

    /// Push a prepend-left whose content is produced by a `write!`-style
    /// format into the reusable scratch buffer, then bump-allocated once.
    ///
    /// Output-equivalent to `prepend_alloc(pos, &format!(...))`.
    #[inline]
    pub fn prepend_fmt(&mut self, pos: u32, args: std::fmt::Arguments<'_>) {
        let allocated = self.alloc_fmt(args);
        self.prepends.push((pos, allocated));
    }

    /// Push a source-mapped prepend-left whose content is produced by a
    /// `write!`-style format into the reusable scratch buffer. The source map
    /// token is placed at the start of the content (offset 0).
    ///
    /// Output-equivalent to `prepend_alloc_mapped(pos, source_pos, &format!(...))`.
    #[allow(dead_code)]
    #[inline]
    pub fn prepend_fmt_mapped(&mut self, pos: u32, source_pos: u32, args: std::fmt::Arguments<'_>) {
        let allocated = self.alloc_fmt(args);
        self.push_mapped(pos, source_pos, 0, allocated);
    }

    /// Push a source-mapped prepend-left with an explicit content offset whose
    /// content is produced by a `write!`-style format into the reusable
    /// scratch buffer. Characters before `content_offset` are unmapped; the
    /// source map token is placed at `content_offset`, pointing to `source_pos`.
    ///
    /// Output-equivalent to
    /// `prepend_alloc_mapped_with_offset(pos, source_pos, content_offset, &format!(...))`.
    #[allow(dead_code)]
    #[inline]
    pub fn prepend_fmt_mapped_with_offset(
        &mut self,
        pos: u32,
        source_pos: u32,
        content_offset: u32,
        args: std::fmt::Arguments<'_>,
    ) {
        let allocated = self.alloc_fmt(args);
        self.push_mapped(pos, source_pos, content_offset, allocated);
    }

    /// Lower a [`MappedGeneratedText`] segment plan at `pos`: each source
    /// segment maps its first byte to the recorded file offset; each synthetic
    /// segment emits no source-map token. The concatenation of the segments
    /// equals `mgt.text`, so the inserted bytes are identical to emitting the
    /// flat string — only the source-map tokens become per-segment precise.
    ///
    /// This is the sole sourcemap-aware lowering for resolved condition / IIFE
    /// expression heads: scaffolding (`__props.`, `.value`, brackets, the
    /// `(` … `) ? ` wrapper, shorthand keys) stays unmapped, only authored
    /// identifiers and verbatim source runs carry tokens.
    #[inline]
    pub fn prepend_mapped_generated_text(&mut self, pos: u32, mgt: &MappedGeneratedText) {
        for seg in &mgt.segments {
            let content = &mgt.text[seg.generated_start as usize..seg.generated_end as usize];
            self.push_prepend_segment(pos, content, seg.source_start);
        }
    }

    /// Record one prepend segment into the shared `mapped_prepends` channel.
    /// `Some(source_pos)` places a single token at the content start; `None`
    /// uses the content-offset-past-end form the source-map emitter treats as
    /// fully unmapped (mirroring [`prepend_ordered_unmapped`](Self::prepend_ordered_unmapped)).
    #[inline]
    fn push_prepend_segment(&mut self, pos: u32, content: &str, source: Option<u32>) {
        if content.is_empty() {
            return;
        }
        let allocated = self.alloc.alloc_str(content);
        match source {
            // Source-derived: token at the segment start (offset 0).
            Some(source_pos) => self.push_mapped(pos, source_pos, 0, allocated),
            // Synthetic: content_offset == len → the emitter places the whole
            // run as an unmapped prefix and emits no source token.
            None => {
                let len = allocated.len() as u32;
                self.mapped_prepends.push((pos, 0, len, allocated));
            }
        }
    }

    /// Move the EDITED content of `[start, end)` to `target`: the run carries
    /// the replacements inside the range and the insertions at its start, as
    /// they stand once every replacement and insertion is applied. The range
    /// is left empty.
    #[inline]
    pub fn relocate(&mut self, start: u32, end: u32, target: u32, prefix: &str, suffix: &str) {
        let prefix = self.alloc.alloc_str(prefix);
        let suffix = self.alloc.alloc_str(suffix);
        self.relocate_allocated(start, end, target, prefix, suffix);
    }

    /// Relocate with already-allocated wrappers; their generated markers
    /// retain the same content identity through application.
    pub(crate) fn relocate_allocated(
        &mut self,
        start: u32,
        end: u32,
        target: u32,
        prefix: &'alloc str,
        suffix: &'alloc str,
    ) {
        self.relocations.push(Relocation {
            start,
            end,
            target,
            prefix,
            suffix,
            intent: RelocationIntent::Relocate,
        });
    }

    /// Emit the AUTHORED bytes of `[start, end)` at `target`, wrapped in
    /// unmapped `prefix` and `suffix`, whatever other edits do to the range.
    /// The range itself is not removed: a producer that wants it gone records
    /// that replacement too.
    #[inline]
    pub fn extract_original(
        &mut self,
        start: u32,
        end: u32,
        target: u32,
        prefix: &str,
        suffix: &str,
    ) {
        let prefix = self.alloc.alloc_str(prefix);
        let suffix = self.alloc.alloc_str(suffix);
        self.relocations.push(Relocation {
            start,
            end,
            target,
            prefix,
            suffix,
            intent: RelocationIntent::ExtractOriginal,
        });
    }

    /// Allocate a string in the bump allocator.
    #[inline]
    pub fn alloc_str(&self, s: &str) -> &'alloc str {
        self.alloc.alloc_str(s)
    }

    /// Allocate a `SegmentAnchor` slice in the bump allocator — the anchor
    /// carrier for Vapor dynamic text parts and the analogous SSR/VDOM
    /// interpolation-anchor call sites.
    #[inline]
    pub fn alloc_segment_anchors(&self, anchors: &[SegmentAnchor]) -> &'alloc [SegmentAnchor] {
        if anchors.is_empty() {
            &[]
        } else {
            self.alloc.alloc_slice_copy(anchors)
        }
    }

    /// Current capacity of the reusable format-sink scratch buffer.
    /// Lets tests prove the buffer is reused (capacity retained) across
    /// formatted emissions rather than reallocated per call.
    #[cfg(test)]
    #[inline]
    pub fn scratch_capacity(&self) -> usize {
        self.scratch.capacity()
    }

    /// Push the leading helper-import preamble as an unmapped insertion and
    /// record it, so source-map generation can report the generated position
    /// immediately after it — the typed helper-import-preamble end boundary
    /// the LSP auto-import classifier reads. For output it is exactly
    /// [`prepend_allocated`](Self::prepend_allocated); the imports stay
    /// unmapped synthetic text.
    pub fn prepend_helper_preamble(
        &mut self,
        pos: u32,
        carrier_anchor: Option<u32>,
        content: &str,
    ) {
        let allocated = self.alloc_str(content);
        self.prepend_allocated(pos, allocated);
        self.helper_preamble = Some((carrier_anchor, allocated));
    }

    /// Apply every recorded edit to `ct`.
    ///
    /// The order is fixed, whatever order the edits were recorded in:
    /// replacements (resolved by one law across all three kinds), extractions
    /// of authored text, insertions, then relocations. A conflict between
    /// replacements or an insertion outside the source or inside a character
    /// is found before anything is applied; every other refusal is the
    /// transform's own typed error.
    #[cfg_attr(feature = "semantic-observe", hotpath::measure)]
    pub fn apply<M: verter_audit::observe::ObserveExec>(
        mut self,
        ct: &mut CodeTransform<'alloc, M>,
    ) -> Result<(), CodeTransformError> {
        if self.map_mode != ct.map_mode() {
            return Err(CodeTransformError::MapNotCaptured);
        }
        // The same `&'alloc str` becomes an inserted chunk below, so
        // source-map generation locates the preamble by pointer.
        if let Some((carrier_anchor, preamble)) = self.helper_preamble.take() {
            match carrier_anchor {
                Some(anchor) => ct.set_helper_preamble_content_at(preamble, anchor),
                None => ct.set_helper_preamble_content(preamble),
            }
        }
        for &(pos, _) in &self.prepends {
            ct.check_offset(pos)?;
        }
        for &(pos, source_pos, _, _) in &self.mapped_prepends {
            ct.check_offset(pos)?;
            ct.check_offset(source_pos)?;
        }
        let replacements = resolve_replacements(&mut self.replacements)?;

        let mapped: SmallVec<[(u32, u32, &'alloc str); 32]> = replacements
            .iter()
            .filter(|replacement| replacement.kind == ReplacementKind::Mapped)
            .map(|replacement| (replacement.start, replacement.end, replacement.content))
            .collect();
        ct.try_batch_overwrite(&mapped)?;
        for replacement in replacements {
            match replacement.kind {
                ReplacementKind::Mapped => {}
                // Synthetic text over an empty range replaces nothing and has
                // no position of its own to stand at.
                ReplacementKind::Unmapped if replacement.start == replacement.end => {}
                ReplacementKind::Unmapped => {
                    ct.try_overwrite_unmapped_alloc(
                        replacement.start,
                        replacement.end,
                        replacement.content,
                    )?;
                }
                ReplacementKind::Segmented(anchors) => {
                    let correspondence = ct.check_correspondence(replacement.content, anchors)?;
                    ct.try_overwrite_segmented(replacement.start, replacement.end, correspondence)?;
                }
            }
        }

        // Extracted authored text lands at its target ahead of the
        // insertions recorded there: it reads the source, so the replacements
        // before it change nothing it emits.
        for relocation in self
            .relocations
            .iter()
            .filter(|relocation| relocation.intent == RelocationIntent::ExtractOriginal)
        {
            ct.try_extract_original(
                relocation.start,
                relocation.end,
                relocation.target,
                relocation.prefix,
                relocation.suffix,
            )?;
        }

        // Reservations that were never filled carry no text.
        self.mapped_prepends
            .retain(|&(_, _, _, content)| !content.is_empty());
        // Stable sorts keep record order for insertions at one position:
        // scope-close suffixes (e.g., ` : _createCommentVNode(...)`) are
        // pushed before sibling comma separators during tree walking, and
        // both land at the element's end position.
        self.prepends.sort_by_key(|&(pos, _)| pos);
        if self.mapped_prepends.is_empty() {
            ct.batch_prepend_left_static(&self.prepends);
        } else {
            // Merge the unmapped (`prepends`) and source-mapped
            // (`mapped_prepends`) channels during one chunk rebuild; at an
            // equal position every unmapped prepend comes first.
            self.mapped_prepends.sort_by_key(|&(pos, ..)| pos);
            ct.batch_prepend_left_merged(&self.prepends, &self.mapped_prepends);
        }

        // Relocated runs carry the edited content of their range, so they
        // move once every replacement and insertion is in place.
        for relocation in self
            .relocations
            .iter()
            .filter(|relocation| relocation.intent == RelocationIntent::Relocate)
        {
            ct.try_move_wrapped_allocated(
                relocation.start,
                relocation.end,
                relocation.target,
                relocation.prefix,
                relocation.suffix,
            )?;
        }

        // Explicit source-map boundaries (read at `generate_map`, after
        // `apply`). Out-of-bounds/mid-codepoint degrades to no boundary.
        for &offset in &self.sourcemap_locations {
            let _ = ct.try_add_sourcemap_location(offset);
        }
        Ok(())
    }
}

/// Resolve every recorded replacement by the one replacement law: sort by
/// start ascending, end descending and record index ascending; drop a
/// replacement that lies inside an accepted one (equal ranges included, so
/// the first recorded wins); refuse any other overlap.
fn resolve_replacements<'alloc>(
    replacements: &mut [Replacement<'alloc>],
) -> Result<SmallVec<[Replacement<'alloc>; 32]>, CodeTransformError> {
    // A stable sort keeps record order among equal ranges.
    replacements.sort_by(|a, b| a.start.cmp(&b.start).then(b.end.cmp(&a.end)));
    let mut accepted: SmallVec<[Replacement<'alloc>; 32]> = SmallVec::new();
    let mut covering: Option<Span> = None;
    for replacement in replacements.iter() {
        let range = Span::new(replacement.start, replacement.end);
        match covering {
            Some(first) if range.start < first.end => {
                if range.end <= first.end {
                    continue;
                }
                return Err(CodeTransformError::Conflict {
                    first,
                    second: range,
                });
            }
            _ => {
                covering = Some(range);
                accepted.push(*replacement);
            }
        }
    }
    Ok(accepted)
}

/// Anchors for one segmented overwrite. Under [`MapMode::NoMap`] a push
/// records nothing, so a producer builds its anchors unconditionally and a
/// text-only recorder captures none of them.
#[derive(Debug, Clone)]
pub struct AnchorBuf {
    anchors: SmallVec<[SegmentAnchor; 4]>,
    map_mode: MapMode,
}

impl AnchorBuf {
    /// Record one anchor; a no-op under [`MapMode::NoMap`].
    #[inline]
    pub fn push(&mut self, anchor: SegmentAnchor) {
        if self.map_mode == MapMode::Map {
            note_map_capture();
            self.anchors.push(anchor);
        }
    }
}

impl std::ops::Deref for AnchorBuf {
    type Target = [SegmentAnchor];

    fn deref(&self) -> &[SegmentAnchor] {
        &self.anchors
    }
}

// ======================== MappedGeneratedText ========================

/// One contiguous run of generated text within a [`MappedGeneratedText`].
///
/// `[generated_start, generated_end)` indexes into the owning plan's `text`.
/// A `Some(source_start)` run is an authored source token whose first byte
/// maps to that file offset; a `None` run is resolver scaffolding (`__props.`,
/// `.value`, `["`, `"]`, the `(` … `) ? ` wrapper, shorthand keys) that must
/// NEVER carry a source-map token.
#[derive(Debug, Clone)]
pub struct MappedSegment {
    pub generated_start: u32,
    pub generated_end: u32,
    pub source_start: Option<u32>,
}

/// A resolved generated expression plus an ordered plan classifying each byte
/// run as authored source (mapped) or synthetic scaffolding (unmapped).
///
/// The concatenation of every segment's slice equals `text` exactly, so a
/// consumer emits byte-identical output whether it uses the flat `text` or the
/// segment plan — the plan only refines the source map. Producers
/// (`build_prefixed_expr_segments` / `resolve_simple_expr_segments`) record one
/// source segment per identifier or verbatim run and one synthetic segment per
/// inserted prefix / suffix / bracket / wrapper, upholding the invariant that
/// no synthetic byte ever maps to source. Lowered via
/// [`EditRecorder::prepend_mapped_generated_text`].
#[derive(Debug, Clone, Default)]
pub struct MappedGeneratedText {
    pub text: String,
    pub segments: SmallVec<[MappedSegment; 8]>,
}

impl MappedGeneratedText {
    /// A single authored-source run mapping its first byte to `source_start`.
    /// Empty `text` yields an empty plan.
    pub fn source(text: &str, source_start: u32) -> Self {
        let mut mgt = Self {
            text: String::new(),
            segments: SmallVec::new(),
        };
        mgt.push(text, Some(source_start));
        mgt
    }

    /// A single synthetic run with no source mapping. Empty `text` yields an
    /// empty plan.
    pub fn synthetic(text: &str) -> Self {
        let mut mgt = Self {
            text: String::new(),
            segments: SmallVec::new(),
        };
        mgt.push(text, None);
        mgt
    }

    /// Append `text` as one segment, classified by `source_start`. Empty input
    /// is skipped so the plan never carries zero-length segments.
    pub(crate) fn push(&mut self, text: &str, source_start: Option<u32>) {
        if text.is_empty() {
            return;
        }
        let generated_start = self.text.len() as u32;
        self.text.push_str(text);
        let generated_end = self.text.len() as u32;
        self.segments.push(MappedSegment {
            generated_start,
            generated_end,
            source_start,
        });
    }

    /// Wrap the plan with an unmapped `prefix` and `suffix`, shifting the inner
    /// segment offsets. Used to add the `(` … `) ? ` ternary head around a
    /// resolved condition expression while keeping the wrapper synthetic.
    pub fn wrapped(&self, prefix: &str, suffix: &str) -> Self {
        let mut text = String::with_capacity(prefix.len() + self.text.len() + suffix.len());
        let mut segments = SmallVec::with_capacity(self.segments.len() + 2);

        if !prefix.is_empty() {
            text.push_str(prefix);
            segments.push(MappedSegment {
                generated_start: 0,
                generated_end: prefix.len() as u32,
                source_start: None,
            });
        }

        let shift = prefix.len() as u32;
        for seg in &self.segments {
            segments.push(MappedSegment {
                generated_start: seg.generated_start + shift,
                generated_end: seg.generated_end + shift,
                source_start: seg.source_start,
            });
        }
        text.push_str(&self.text);

        if !suffix.is_empty() {
            let generated_start = text.len() as u32;
            text.push_str(suffix);
            segments.push(MappedSegment {
                generated_start,
                generated_end: text.len() as u32,
                source_start: None,
            });
        }

        Self { text, segments }
    }
}

#[cfg(test)]
#[path = "recorder_tests.rs"]
mod tests;
