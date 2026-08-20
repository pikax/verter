//! Code generation output accumulator and internal types.
//!
//! All codegen operations are deferred into [`CodeGenOutput`] vecs.
//! Nothing is applied to the source until [`CodeGenOutput::apply_to()`] is called.

use oxc_allocator::Allocator;
use smallvec::SmallVec;

use crate::code_transform::{CodeTransform, SegmentAnchor};

use super::vapor::PendingNavQueue;

use super::shared::helpers::{
    BuiltinComponentFlags, SsrHelper, SsrHelperFlags, VaporHelper, VaporHelperFlags, VdomHelper,
    VdomHelperFlags,
};

/// Capability token for [`CodeTransform::try_overwrite_segmented`].
/// Constructor is `pub(in crate::template::code_gen)` — only Vue
/// VDOM/Vapor/SSR emitters can mint one. `ide`/`svelte` get `E0603`.
/// `apply_to` mints its own only to drain already-authorized entries.
///
/// External-crate unreachability:
/// `tests/cases/segmented_overwrite_compile_fail.rs`. Intra-crate
/// exclusion of `ide`/`svelte` is the privacy checker, not a scanner.
#[derive(Clone, Copy)]
pub struct SegmentedOverwriteAuthority(());

impl SegmentedOverwriteAuthority {
    pub(in crate::template::code_gen) fn new() -> Self {
        Self(())
    }

    /// Test-only hatch for `code_transform/segmented_tests.rs` (outside
    /// `template::code_gen`). Present under `cfg(test)` only; production
    /// builds keep the call-site restriction.
    #[cfg(test)]
    pub(crate) fn new_for_test() -> Self {
        Self(())
    }
}

// ======================== CodeGenOutput ========================

/// Accumulated code generation operations.
///
/// All operations are deferred — nothing is applied until [`apply_to()`](Self::apply_to).
/// This avoids passing `CodeTransform` through every trait method and enables
/// a single O(n+m) batch application at the end.
#[derive(Clone)]
pub struct CodeGenOutput<'alloc> {
    /// Replace source ranges: (start, end, replacement).
    /// Applied via `ct.batch_overwrite()` in sorted order.
    pub overwrites: Vec<(u32, u32, &'alloc str)>,

    /// Replace source ranges with wholly synthetic content that has no
    /// character-level correspondence to the replaced span: (start, end,
    /// replacement). Applied via `ct.overwrite_unmapped()` per entry — see
    /// [`overwrite_or_root_prefix`](Self::overwrite_or_root_prefix) for why
    /// this is a separate channel from `overwrites` rather than a flag on
    /// the same one.
    unmapped_overwrites: Vec<(u32, u32, &'alloc str)>,

    /// Segmented overwrite: (start, end, replacement, anchors). Applied via
    /// `ct.try_overwrite_segmented()`. Vue VDOM/Vapor/SSR only — interpolation
    /// identifiers and static attribute names inside otherwise synthetic code.
    segmented_overwrites: Vec<(u32, u32, &'alloc str, &'alloc [SegmentAnchor])>,

    /// Insert content before a position: (position, content).
    /// Applied via `ct.batch_prepend_left_static()` in sorted order.
    /// Used for binding prefixes (`_ctx.`, `$setup.`), suffixes (`.value`), separators.
    pub prepends: Vec<(u32, &'alloc str)>,

    /// Insert content before a position with source map mapping:
    /// (insertion_pos, source_pos, content_offset, content).
    /// Creates `InsertedMapped` chunks that emit source map tokens at `source_pos`.
    /// `content_offset` shifts the token within the content (characters before it
    /// are unmapped). Used for relocated directive expressions (v-if conditions,
    /// v-for iterables) where binding prefixes precede the original identifier.
    pub mapped_prepends: Vec<(u32, u32, u32, &'alloc str)>,

    /// VDOM runtime helper imports (bitflags, O(1) dedup).
    vdom_imports: VdomHelperFlags,

    /// Vapor runtime helper imports (bitflags, O(1) dedup).
    vapor_imports: VaporHelperFlags,

    /// SSR runtime helper imports from `vue/server-renderer` (bitflags, O(1) dedup).
    ssr_imports: SsrHelperFlags,

    /// Vue built-in component imports (Suspense, Teleport, etc.).
    builtin_imports: BuiltinComponentFlags,

    /// Deferred move operations: (start, end, target).
    /// Applied via `ct.move_slice()` after overwrites and prepends.
    moves: Vec<(u32, u32, u32)>,

    /// Deferred wrapped move operations: (start, end, target, prefix, suffix).
    /// Applied via `ct.move_wrapped()` after overwrites and prepends.
    /// Preserves sourcemap for the moved content while wrapping it.
    wrapped_moves: Vec<(u32, u32, u32, &'alloc str, &'alloc str)>,

    /// The bump-allocated content of the leading helper-import preamble insertion, when codegen
    /// emitted one via [`prepend_helper_preamble`](Self::prepend_helper_preamble). Transferred to
    /// the [`CodeTransform`] in [`apply_to`](Self::apply_to) so source-map generation can report
    /// the generated-TSX position immediately after it (the typed preamble-end boundary). `None`
    /// when no helper-import preamble was emitted.
    helper_preamble: Option<&'alloc str>,

    /// Explicit source-map boundary offsets, transferred in
    /// [`apply_to`](Self::apply_to) via `try_add_sourcemap_location`.
    /// `Original`/`Moved` chunks otherwise token only start and newlines;
    /// a registered offset gets a mid-line token (e.g. a top-level
    /// identifier the `verbatim-carry` oracle must land on).
    sourcemap_locations: Vec<u32>,

    /// Allocator reference for bump-allocating generated strings.
    alloc: &'alloc Allocator,

    /// One reusable scratch buffer for the `write!`-style format sinks
    /// (`overwrite_fmt`, `prepend_fmt`, and the mapped variants). Each sink
    /// clears and reuses it, so a formatted emission costs the retained heap
    /// capacity plus one bump copy — never a fresh `String` per call. It is
    /// an operation-construction helper only; it never holds built output.
    scratch: String,
}

impl<'alloc> CodeGenOutput<'alloc> {
    /// Create a new empty output accumulator.
    pub fn new(alloc: &'alloc Allocator) -> Self {
        Self {
            overwrites: Vec::with_capacity(16),
            unmapped_overwrites: Vec::new(),
            segmented_overwrites: Vec::new(),
            prepends: Vec::with_capacity(16),
            mapped_prepends: Vec::new(),
            vdom_imports: VdomHelperFlags::empty(),
            vapor_imports: VaporHelperFlags::empty(),
            ssr_imports: SsrHelperFlags::empty(),
            builtin_imports: BuiltinComponentFlags::empty(),
            moves: Vec::new(),
            wrapped_moves: Vec::new(),
            helper_preamble: None,
            sourcemap_locations: Vec::new(),
            alloc,
            scratch: String::new(),
        }
    }

    /// Push an overwrite operation. The content is bump-allocated.
    #[inline]
    pub fn overwrite(&mut self, start: u32, end: u32, content: &str) {
        let allocated = self.alloc.alloc_str(content);
        self.overwrites.push((start, end, allocated));
    }

    /// Push an overwrite with pre-allocated content (avoids double allocation).
    #[inline]
    pub fn overwrite_alloc(&mut self, start: u32, end: u32, content: &'alloc str) {
        self.overwrites.push((start, end, content));
    }

    /// Push an unmapped overwrite operation (see `unmapped_overwrites` field
    /// doc). The content is bump-allocated.
    #[inline]
    pub fn overwrite_unmapped(&mut self, start: u32, end: u32, content: &str) {
        let allocated = self.alloc.alloc_str(content);
        self.unmapped_overwrites.push((start, end, allocated));
    }

    /// Segmented overwrite: `content` replaces `[start, end)`; each `anchors`
    /// entry is an authored lexeme inside `content`. Token is required here
    /// (the emitter entry), not only on the primitive `apply_to` drains into —
    /// otherwise any `CodeGenOutput` caller could reach this with no token.
    /// See [`SegmentedOverwriteAuthority`].
    #[inline]
    pub fn overwrite_segmented(
        &mut self,
        start: u32,
        end: u32,
        content: &str,
        anchors: &[SegmentAnchor],
        _authority: SegmentedOverwriteAuthority,
    ) {
        let content_ref = self.alloc.alloc_str(content);
        let anchors_ref: &'alloc [SegmentAnchor] = if anchors.is_empty() {
            &[]
        } else {
            self.alloc.alloc_slice_copy(anchors)
        };
        self.segmented_overwrites
            .push((start, end, content_ref, anchors_ref));
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

    /// Root wrapper insertion for a raw template unit whose synthetic root has
    /// a zero-width opening tag. It must precede child operations at the same
    /// byte boundary.
    ///
    /// Nonzero-width (a real tag, e.g. `<script setup>`, replaced by synthetic
    /// wrapper) uses an unmapped overwrite: still `Overwritten` (ordering vs
    /// later `move_slice` stays intact) but no source-map token. A mapped
    /// `Overwritten` would claim the tag's start for the entire replacement —
    /// truthful only for 1:1 substitutions. Official rc.3 `prependLeft` wrapper
    /// insertion never touches the tag and carries no mapping either.
    pub fn overwrite_or_root_prefix(&mut self, start: u32, end: u32, content: &str) {
        if start == end {
            let allocated = self.alloc.alloc_str(content);
            self.prepends.insert(0, (start, allocated));
        } else {
            self.overwrite_unmapped(start, end, content);
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
        authority: SegmentedOverwriteAuthority,
    ) {
        if start == end {
            let allocated = self.alloc.alloc_str(content);
            self.prepends.insert(0, (start, allocated));
        } else {
            self.overwrite_segmented(start, end, content, anchors, authority);
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

    /// Push the leading helper-import preamble as an (unmapped) prepend-left AND record its content
    /// reference so source-map generation can report the generated-TSX position immediately after
    /// it — the typed helper-import-preamble end boundary consumed by the LSP auto-import
    /// classifier. Behaves exactly like [`prepend_alloc`](Self::prepend_alloc) for output (the
    /// imports themselves stay unmapped synthetic text); the only addition is the recorded identity.
    /// Called once per IDE script generation from `emit_helper_imports`.
    #[inline]
    pub fn prepend_helper_preamble(&mut self, pos: u32, content: &str) {
        let allocated = self.alloc.alloc_str(content);
        self.prepends.push((pos, allocated));
        self.helper_preamble = Some(allocated);
    }

    /// Register an explicit source-map boundary at an original-source byte
    /// offset (see `sourcemap_locations` field doc). A no-op for source-map
    /// purposes when the position ends up outside any surviving `Original`/
    /// `Moved` chunk or mid-codepoint — `CodeTransform::try_add_sourcemap_location`
    /// degrades gracefully rather than corrupting the map.
    #[inline]
    pub fn add_sourcemap_location(&mut self, offset: u32) {
        self.sourcemap_locations.push(offset);
    }

    /// Push a source-mapped prepend-left with bump-allocated content.
    /// The generated chunk maps back to `source_pos` in the source map.
    /// The source map token is placed at the start of the content (offset 0).
    #[inline]
    pub fn prepend_alloc_mapped(&mut self, pos: u32, source_pos: u32, content: &str) {
        let allocated = self.alloc.alloc_str(content);
        self.mapped_prepends.push((pos, source_pos, 0, allocated));
    }

    /// Push an UNMAPPED prepend-left that interleaves with mapped prepends at the
    /// same position in insertion order.
    ///
    /// Plain [`prepend_static`](Self::prepend_static) / [`prepend_alloc`](Self::prepend_alloc)
    /// land in the `prepends` vec, which `apply_to` concatenates BEFORE all
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
        self.mapped_prepends
            .push((pos, source_pos, content_offset, allocated));
    }

    /// Write `args` into the reusable scratch buffer and bump-allocate the
    /// result once. The buffer is cleared first and its capacity is retained
    /// for the next emission, so repeated formatted emissions reuse one
    /// allocation instead of allocating a fresh `String` per call.
    ///
    /// This is the arena-string companion to the op-recording format sinks
    /// (`overwrite_fmt` / `prepend_fmt` / …): those record a `CodeTransform`
    /// operation, whereas this returns the bump-allocated `&'alloc str` for
    /// content that is stored and assembled later rather than applied at a
    /// source position — e.g. the Vapor navigation (`const pN = _child(nM)`)
    /// and text-creation (`const xN = _txt(pP)`) lines accumulated for the
    /// render-function body. It produces a string through the SAME reusable
    /// scratch + single-bump path, never by editing already-emitted output in
    /// place.
    #[inline]
    pub(in crate::template::code_gen) fn alloc_fmt(
        &mut self,
        args: std::fmt::Arguments<'_>,
    ) -> &'alloc str {
        use std::fmt::Write as _;
        self.scratch.clear();
        // Appending to a `String` never fails; `write_fmt` can only return
        // `Err` if a `Display`/`Debug` impl inside `args` does. Surface that
        // as a panic rather than silently recording partial content.
        self.scratch
            .write_fmt(args)
            .expect("CodeGenOutput format sink: Display impl must not fail");
        self.alloc.alloc_str(&self.scratch)
    }

    /// Push an overwrite whose content is produced by a `write!`-style format
    /// directly into the reusable scratch buffer, then bump-allocated once.
    ///
    /// Output-equivalent to `overwrite(start, end, &format!(...))` but avoids
    /// the intermediate per-call `String` allocation. The recorded operation
    /// is an ordinary overwrite — only the string PRODUCTION changes.
    #[inline]
    pub fn overwrite_fmt(&mut self, start: u32, end: u32, args: std::fmt::Arguments<'_>) {
        let allocated = self.alloc_fmt(args);
        self.overwrites.push((start, end, allocated));
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
    ///
    /// Rounds out the format-sink family alongside its `*_with_offset` peer;
    /// exercised by the format-sink equivalence tests.
    #[allow(dead_code)]
    #[inline]
    pub fn prepend_fmt_mapped(&mut self, pos: u32, source_pos: u32, args: std::fmt::Arguments<'_>) {
        let allocated = self.alloc_fmt(args);
        self.mapped_prepends.push((pos, source_pos, 0, allocated));
    }

    /// Push a source-mapped prepend-left with an explicit content offset whose
    /// content is produced by a `write!`-style format into the reusable
    /// scratch buffer. Characters before `content_offset` are unmapped; the
    /// source map token is placed at `content_offset`, pointing to `source_pos`.
    ///
    /// Output-equivalent to
    /// `prepend_alloc_mapped_with_offset(pos, source_pos, content_offset, &format!(...))`.
    ///
    /// Rounds out the format-sink family alongside its zero-offset peer;
    /// exercised by the format-sink equivalence tests.
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
        self.mapped_prepends
            .push((pos, source_pos, content_offset, allocated));
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
            Some(source_pos) => self.mapped_prepends.push((pos, source_pos, 0, allocated)),
            // Synthetic: content_offset == len → the emitter places the whole
            // run as an unmapped prefix and emits no source token.
            None => {
                let len = allocated.len() as u32;
                self.mapped_prepends.push((pos, 0, len, allocated));
            }
        }
    }

    /// Push a move operation: move source range [start, end) to target position.
    #[inline]
    pub fn move_slice(&mut self, start: u32, end: u32, target: u32) {
        self.moves.push((start, end, target));
    }

    /// Push a wrapped move operation: move source range [start, end) to target
    /// position with unmapped prefix and suffix.
    ///
    /// The moved content preserves its sourcemap mapping, while the prefix and
    /// suffix are unmapped insertions. Used for wrapping type content in
    /// declarations while preserving fine-grained hover resolution.
    #[inline]
    pub fn move_wrapped(&mut self, start: u32, end: u32, target: u32, prefix: &str, suffix: &str) {
        let p = self.alloc.alloc_str(prefix);
        let s = self.alloc.alloc_str(suffix);
        self.wrapped_moves.push((start, end, target, p, s));
    }

    /// Allocate a string in the bump allocator.
    #[inline]
    pub fn alloc_str(&self, s: &str) -> &'alloc str {
        self.alloc.alloc_str(s)
    }

    /// Allocate a `SegmentAnchor` slice in the bump allocator — the anchor
    /// carrier for [`VaporTextPart::Dynamic`] and analogous SSR/VDOM
    /// interpolation-anchor call sites.
    #[inline]
    pub fn alloc_segment_anchors(&self, anchors: &[SegmentAnchor]) -> &'alloc [SegmentAnchor] {
        if anchors.is_empty() {
            &[]
        } else {
            self.alloc.alloc_slice_copy(anchors)
        }
    }

    /// Record a VDOM runtime helper import.
    #[inline]
    pub fn add_vdom_import(&mut self, h: VdomHelper) {
        self.vdom_imports = self.vdom_imports.add(h);
    }

    /// Record a Vapor runtime helper import.
    #[inline]
    pub fn add_vapor_import(&mut self, h: VaporHelper) {
        self.vapor_imports = self.vapor_imports.add(h);
    }

    /// Record a Vue built-in component import (Suspense, Teleport, etc.).
    #[inline]
    pub fn add_builtin_component(&mut self, flag: u8) {
        self.builtin_imports = self.builtin_imports.add(flag);
    }

    /// Record an SSR runtime helper import (from `vue/server-renderer`).
    #[inline]
    pub fn add_ssr_import(&mut self, h: SsrHelper) {
        self.ssr_imports = self.ssr_imports.add(h);
    }

    /// Read-only access to VDOM import flags.
    #[cfg(test)]
    #[inline]
    pub fn vdom_imports(&self) -> VdomHelperFlags {
        self.vdom_imports
    }

    /// Read-only access to Vapor import flags.
    #[cfg(test)]
    #[inline]
    pub fn vapor_imports(&self) -> VaporHelperFlags {
        self.vapor_imports
    }

    /// Current capacity of the reusable format-sink scratch buffer.
    /// Lets tests prove the buffer is reused (capacity retained) across
    /// formatted emissions rather than reallocated per call.
    #[cfg(test)]
    #[inline]
    pub fn scratch_capacity(&self) -> usize {
        self.scratch.capacity()
    }

    /// Sort and apply all accumulated operations to a CodeTransform.
    /// Called once after the entire tree walk.
    ///
    /// Returns the categorized runtime helper imports collected during codegen.
    /// Vue helpers go to `vue`, SSR helpers go to `ssr` (from `vue/server-renderer`).
    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    pub fn apply_to(mut self, ct: &mut CodeTransform<'alloc>) -> TemplateImports {
        // Carry the recorded helper-import preamble identity into the transform. The same `&'alloc
        // str` becomes an `Inserted` chunk below, so source-map generation can locate it by pointer
        // and report the typed preamble-end boundary. No-op when no preamble was emitted.
        if let Some(preamble) = self.helper_preamble {
            ct.set_helper_preamble_content(preamble);
        }

        // Apply wrapped moves FIRST — they operate on Original chunks and must
        // run before overwrites replace those chunks. This preserves sourcemap
        // for moved content (e.g., defineProps type params).
        for &(start, end, target, prefix, suffix) in &self.wrapped_moves {
            ct.move_wrapped(start, end, target, prefix, suffix);
        }

        // Sort by start ascending, then by end descending (so that for equal
        // starts, the wider range comes first and the narrower is filtered out).
        self.overwrites
            .sort_unstable_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));

        // Filter out fully-contained ranges. After sorting, any range whose
        // end <= the running max_end is fully inside a preceding range and
        // would produce a redundant (overlapping) overwrite.
        let mut max_end: u32 = 0;
        self.overwrites.retain(|&(start, end, _)| {
            if start >= max_end {
                // Non-overlapping — accept and update max_end
                max_end = end;
                true
            } else if end > max_end {
                // Partial overlap extending beyond max_end — accept the extension
                max_end = end;
                true
            } else {
                // Fully contained (start >= prev_start && end <= max_end) — drop
                false
            }
        });

        ct.batch_overwrite(&self.overwrites);

        // Unmapped overwrites are few and never overlap `overwrites` (both
        // target root-level tag spans) — a sorted loop is enough.
        self.unmapped_overwrites
            .sort_unstable_by_key(|&(start, ..)| start);
        for &(start, end, content) in &self.unmapped_overwrites {
            ct.overwrite_unmapped(start, end, content);
        }

        // Segmented overwrites: disjoint from the other overwrite channels
        // (one mechanism per AST node), so ordering vs those channels is
        // irrelevant. Failure is a caller precondition violation, not
        // recoverable. Fail closed in every build: a silent `ct.overwrite()`
        // fallback would re-fabricate whole-block provenance (every byte of
        // `content` mapping to `start`) — a silently wrong source map.
        self.segmented_overwrites
            .sort_unstable_by_key(|&(start, ..)| start);
        for &(start, end, content, anchors) in &self.segmented_overwrites {
            if let Err(err) = ct.try_overwrite_segmented(
                start,
                end,
                content,
                anchors,
                SegmentedOverwriteAuthority::new(),
            ) {
                panic!("overwrite_segmented precondition violated at [{start},{end}): {err:?}");
            }
        }

        if self.mapped_prepends.is_empty() {
            // Fast path: no mapped prepends, use the simpler batch method.
            // Must use stable sort to preserve insertion order for same-position
            // prepends. Scope-close suffixes (e.g., ` : _createCommentVNode(...)`)
            // are pushed before sibling comma separators during tree walking, and
            // both land at the element's end position. Unstable sort can reorder
            // them, producing invalid JS like `, : _createCommentVNode(...)`.
            self.prepends.sort_by_key(|(pos, _)| *pos);
            ct.batch_prepend_left_static(&self.prepends);
        } else {
            // Merge the unmapped (`prepends`) and source-mapped (`mapped_prepends`)
            // channels DIRECTLY during one chunk rebuild — no third temporary Vec.
            // Each channel is stably sorted by position; the merge emits every
            // unmapped prepend before any mapped prepend at an equal position, so
            // the two channels interleave at a shared anchor in unmapped-first order.
            self.prepends.sort_by_key(|&(pos, _)| pos);
            self.mapped_prepends.sort_by_key(|&(pos, ..)| pos);
            ct.batch_prepend_left_merged(&self.prepends, &self.mapped_prepends);
        }

        // Apply deferred move operations (e.g., slot reordering)
        for &(start, end, target) in &self.moves {
            ct.move_slice(start, end, target);
        }

        // Explicit source-map boundaries (read at `generate_map`, after
        // `apply_to`). Out-of-bounds/mid-codepoint degrades to no boundary.
        for &offset in &self.sourcemap_locations {
            let _ = ct.try_add_sourcemap_location(offset);
        }

        // Return whichever mode's imports are non-empty (only one is ever active)
        let mut vue = if !self.vdom_imports.is_empty() {
            self.vdom_imports.to_imports()
        } else {
            self.vapor_imports.to_imports()
        };

        // Append built-in component imports (Suspense, Teleport, etc.)
        if !self.builtin_imports.is_empty() {
            vue.extend(self.builtin_imports.to_imports());
        }

        let ssr = self.ssr_imports.to_imports();

        TemplateImports { vue, ssr }
    }
}

// ======================== TemplateImports ========================

/// Categorized runtime helper imports from template codegen.
///
/// Separates Vue helpers (from `"vue"`) and SSR helpers (from `"vue/server-renderer"`)
/// so the caller can emit two distinct import lines for SSR builds.
pub struct TemplateImports {
    /// Helpers imported from `"vue"` (e.g., `_mergeProps`, `_resolveComponent`).
    pub vue: Vec<&'static str>,
    /// Helpers imported from `"vue/server-renderer"` (e.g., `_ssrRenderAttrs`).
    pub ssr: Vec<&'static str>,
}

impl TemplateImports {
    /// Returns true if there are no imports at all.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.vue.is_empty() && self.ssr.is_empty()
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
/// [`CodeGenOutput::prepend_mapped_generated_text`].
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

// ======================== ChildRecord ========================

/// Record of a child node, used by the parent's leave phase to decide
/// separators, array wrapping, and patch flags.
#[derive(Debug)]
pub struct ChildRecord {
    pub start: u32,
    pub end: u32,
    pub kind: ChildKind,
    /// For element children: whether this element starts or continues a
    /// v-if/v-else-if/v-else chain. `None` for non-element children and
    /// elements without v-if.
    pub condition: Option<ConditionChainRole>,
    /// Condition prefix for v-if/v-else-if elements (e.g. `(show) ? `), carried
    /// as an ordered segment plan so the emitter maps each authored identifier
    /// back to source while leaving the synthetic binding prefixes (`__props.`),
    /// suffixes (`.value`), keyword brackets, and the `(` … `) ? ` wrapper
    /// unmapped. Emitted by the parent's separator logic to ensure correct
    /// ordering relative to comma separators. `None` for non-conditional
    /// elements and v-else (whose `: ` comes from the previous branch's scope
    /// close).
    pub condition_prefix: Option<MappedGeneratedText>,
}

/// Role of an element in a v-if/v-else-if/v-else chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConditionChainRole {
    /// `v-if` — starts a new conditional chain.
    Start,
    /// `v-else-if` or `v-else` — continues the preceding chain.
    Continuation,
}

/// Classification of a child node for the parent's leave phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChildKind {
    /// Plain text content.
    Text,
    /// `{{ expr }}` interpolation.
    Interpolation,
    /// Element or component child.
    Element,
    /// HTML comment.
    Comment,
    /// All-whitespace containing a newline (deferred to parent for context-dependent resolution).
    WhitespaceNewline,
    /// All-whitespace without a newline (deferred to parent for context-dependent resolution).
    WhitespaceSpace,
    /// Fully-static element(s) — retained for potential future SSR use.
    #[allow(dead_code)]
    StaticVNode {
        /// Number of root-level elements in this static group.
        count: u32,
    },
}

// ======================== ScopeClose ========================

/// Scope close markers for structural directives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeClose {
    /// Close a v-if ternary: ` : _createCommentVNode("v-if", true))`
    IfTernary,
    /// Close a v-else-if ternary.
    ElseIfTernary,
    /// Close a v-else branch.
    Else,
    /// Close a v-for renderList.
    ///
    /// Official `isStableFragment = source is SIMPLE_EXPRESSION &&
    /// constType > 0` — iterable is a static `<script setup>` const/import.
    /// Takes priority over `is_keyed` (`isStableFragment ? 64 : keyProp ? 128 : 256`);
    /// per-item elements skip their own block wrap (`is_block_root`'s `v_for` arm).
    For { is_keyed: bool, is_stable: bool },
    /// Close a v-for renderList nested INSIDE a v-if/v-else-if/v-else
    /// branch — both structural directives on ONE element. The condition
    /// stays OUTER (official v-if-over-v-for priority) and the branch
    /// value is the renderList fragment, so the fragment close is
    /// followed immediately by the branch's ternary close.
    ForInCondition {
        is_keyed: bool,
        condition: ConditionBranchClose,
    },
    /// Close a v-slot wrapper.
    #[allow(dead_code)]
    SlotWrapper,
}

/// The condition half of [`ScopeClose::ForInCondition`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConditionBranchClose {
    /// End of chain without an else: ` : _createCommentVNode("v-if", true)`.
    IfTernary,
    /// A continuation follows: ` : `.
    ElseIfTernary,
    /// Terminal v-else branch: nothing after the fragment close.
    Else,
}

// ======================== Vapor-specific types ========================

/// Counter allocator for Vapor variable names.
///
/// Each call to `next_*()` returns the current value and increments.
/// Variable naming: `n0`, `n1` (nodes), `x0` (text), `p0` (path), `t0` (template).
///
/// `n` and `x` share one counter — official `context.reference()` has no
/// separate id space: `const x2 = _txt(n2)` uses the same id as `const n2 = t0()`
/// (rc.3). No `next_text()`; callers use `next_node()` (`ensure_text_ref`).
#[derive(Debug, Default)]
pub struct VaporCounters {
    /// Node/text reference counter (n0, n1, ... — also used for x0, x1, ...).
    pub n: u32,
    /// Navigation path counter (p0, p1, ...).
    pub p: u32,
    /// Template index counter (t0, t1, ...).
    pub t: u32,
}

impl VaporCounters {
    pub fn next_node(&mut self) -> u32 {
        let v = self.n;
        self.n += 1;
        v
    }

    pub fn next_path(&mut self) -> u32 {
        let v = self.p;
        self.p += 1;
        v
    }

    pub fn next_template(&mut self) -> u32 {
        let v = self.t;
        self.t += 1;
        v
    }
}

/// A text part in dynamic text content.
///
/// Vapor collects text and interpolation children into parts, then emits
/// `_setText(xN, part1 + part2 + ...)` inside a `_renderEffect`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VaporTextPart<'a> {
    /// Literal text: `"hello "`.
    Static(&'a str),
    /// Dynamic expression (`_toDisplayString(_ctx.msg)`) plus authored
    /// interpolation anchors relative to this part's start. Empty if none.
    Dynamic(&'a str, &'a [SegmentAnchor]),
}

impl VaporTextPart<'_> {
    /// Format this part as a JS expression fragment.
    pub fn to_js(&self) -> &str {
        match self {
            VaporTextPart::Static(s) => s,
            VaporTextPart::Dynamic(s, _) => s,
        }
    }

    /// Returns true if this is a dynamic part.
    #[cfg(test)]
    pub fn is_dynamic(&self) -> bool {
        matches!(self, VaporTextPart::Dynamic(..))
    }
}

/// A reactive effect to emit in a `_renderEffect` block.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)]
pub enum VaporEffect<'a> {
    /// `_setText(xN, parts...)`.
    SetText {
        text_ref: u32,
        parts: Vec<VaporTextPart<'a>>,
    },
    /// `_setClass(nN, expr)`.
    SetClass { node_ref: u32, expr: &'a str },
    /// `_setStyle(nN, expr)`.
    SetStyle { node_ref: u32, expr: &'a str },
    /// `_setProp(nN, "attr", expr)`.
    SetProp {
        node_ref: u32,
        attr: &'a str,
        expr: &'a str,
    },
    /// `_setAttr(nN, "attr", expr)`.
    SetAttr {
        node_ref: u32,
        attr: &'a str,
        expr: &'a str,
    },
    /// `_setHtml(nN, expr)`.
    SetHtml { node_ref: u32, expr: &'a str },
    /// `_setDynamicProps(nN, [expr])`.
    SetDynamicProps { node_ref: u32, expr: &'a str },
}

impl VaporEffect<'_> {
    /// Write this effect's JS code directly into a buffer, avoiding allocation.
    pub fn write_code_into(&self, buf: &mut String) {
        use super::shared::helpers::push_u32;
        match self {
            VaporEffect::SetText { text_ref, parts } => {
                buf.push_str("_setText(x");
                push_u32(buf, *text_ref);
                buf.push_str(", ");
                for (i, part) in parts.iter().enumerate() {
                    if i > 0 {
                        buf.push_str(" + ");
                    }
                    buf.push_str(part.to_js());
                }
                buf.push(')');
            }
            other => other.write_code_into_tail(buf),
        }
    }

    /// Same bytes as [`write_code_into`](Self::write_code_into), plus absolute
    /// interpolation anchors in `buf`. Only `SetText` [`VaporTextPart::Dynamic`]
    /// parts carry anchors; other variants share the tail writer.
    pub(crate) fn write_code_into_with_anchors(
        &self,
        buf: &mut String,
        anchors: &mut Vec<SegmentAnchor>,
    ) {
        use super::shared::helpers::push_u32;
        match self {
            VaporEffect::SetText { text_ref, parts } => {
                buf.push_str("_setText(x");
                push_u32(buf, *text_ref);
                buf.push_str(", ");
                for (i, part) in parts.iter().enumerate() {
                    if i > 0 {
                        buf.push_str(" + ");
                    }
                    let base = buf.len() as u32;
                    buf.push_str(part.to_js());
                    if let VaporTextPart::Dynamic(_, part_anchors) = part {
                        anchors.extend(part_anchors.iter().map(|a| SegmentAnchor {
                            content_offset: base + a.content_offset,
                            length: a.length,
                            source_pos: a.source_pos,
                        }));
                    }
                }
                buf.push(')');
            }
            other => other.write_code_into_tail(buf),
        }
    }

    /// Shared tail of [`write_code_into`](Self::write_code_into) and
    /// [`write_code_into_with_anchors`](Self::write_code_into_with_anchors) —
    /// no `VaporTextPart`, nothing to anchor.
    fn write_code_into_tail(&self, buf: &mut String) {
        use super::shared::helpers::push_u32;
        match self {
            VaporEffect::SetText { .. } => unreachable!("SetText handled by both callers"),
            VaporEffect::SetClass { node_ref, expr } => {
                buf.push_str("_setClass(n");
                push_u32(buf, *node_ref);
                buf.push_str(", ");
                buf.push_str(expr);
                buf.push(')');
            }
            VaporEffect::SetStyle { node_ref, expr } => {
                buf.push_str("_setStyle(n");
                push_u32(buf, *node_ref);
                buf.push_str(", ");
                buf.push_str(expr);
                buf.push(')');
            }
            VaporEffect::SetProp {
                node_ref,
                attr,
                expr,
            } => {
                buf.push_str("_setProp(n");
                push_u32(buf, *node_ref);
                buf.push_str(", \"");
                buf.push_str(attr);
                buf.push_str("\", ");
                buf.push_str(expr);
                buf.push(')');
            }
            VaporEffect::SetAttr {
                node_ref,
                attr,
                expr,
            } => {
                buf.push_str("_setAttr(n");
                push_u32(buf, *node_ref);
                buf.push_str(", \"");
                buf.push_str(attr);
                buf.push_str("\", ");
                buf.push_str(expr);
                buf.push(')');
            }
            VaporEffect::SetHtml { node_ref, expr } => {
                buf.push_str("_setHtml(n");
                push_u32(buf, *node_ref);
                buf.push_str(", ");
                buf.push_str(expr);
                buf.push(')');
            }
            VaporEffect::SetDynamicProps { node_ref, expr } => {
                buf.push_str("_setDynamicProps(n");
                push_u32(buf, *node_ref);
                buf.push_str(", [");
                buf.push_str(expr);
                buf.push_str("])");
            }
        }
    }

    /// Render this effect as a standalone JS statement string.
    ///
    /// Used when cross-file const prop analysis determines the prop doesn't need
    /// reactive tracking — the setter is emitted as a one-time direct statement
    /// instead of being wrapped in `_renderEffect`.
    pub fn to_statement(&self) -> String {
        let mut buf = String::with_capacity(64);
        self.write_code_into(&mut buf);
        buf
    }

    /// Alias for `to_statement()` used in tests.
    #[cfg(test)]
    pub fn to_code(&self) -> String {
        self.to_statement()
    }
}

/// Per-element state for Vapor codegen.
///
/// Pushed onto the stack when entering an element, popped on leave. Holds this
/// element's render-side output buffers plus the traversal-local DOM child
/// cursor used to index its children. Static facts about the element itself
/// (tag name, is_root, is_void, depth, has_dynamic_text) are derived from the
/// AST on demand in the caller rather than stored here.
#[derive(Debug)]
pub struct VaporElementState<'a> {
    /// Finalized static HTML for this element's subtree. Only template-scope
    /// roots (root elements, components, slot outlets, slot templates) carry
    /// content here, handed over once when the shared scope buffer is closed;
    /// plain descendants append into that shared buffer directly, so their own
    /// field stays empty and is never read.
    pub html: String,
    /// Dynamic text parts for the current text group.
    pub text_parts: Vec<VaporTextPart<'a>>,
    /// Node ref index if this element needs one (Some(N) → `nN`).
    pub node_ref: Option<u32>,
    /// Text node ref index if needed (Some(N) → `xN`).
    pub text_node_ref: Option<u32>,
    /// Effects for this element specifically.
    pub own_effects: Vec<VaporEffect<'a>>,
    /// Navigation instructions from children (bubbled up).
    pub child_nav: Vec<&'a str>,
    /// Text node creation statements from children.
    pub child_text_creations: Vec<&'a str>,
    /// Effects from children (bubbled up).
    pub child_effects: Vec<VaporEffect<'a>>,
    /// Non-effect statements with authored anchors (relative to statement start).
    /// A bubbled v-if/v-for statement can embed an interpolation identifier.
    pub child_statements: Vec<(&'a str, &'a [SegmentAnchor])>,
    /// Named slot closure entries built from `<template v-slot>` children.
    /// Each string is a complete slot entry (e.g., `header: () => { ... }`).
    pub named_slots: Vec<String>,
    /// Running count of this element's DOM children observed so far during the
    /// DFS — i.e. the DOM index the next element child will occupy. Adjacent
    /// text/interpolation nodes coalesce into a single DOM child, comments
    /// count only when rendered, and every element counts once. Maintained
    /// incrementally as each child is walked, replacing a per-child rescan of
    /// the preceding siblings.
    pub dom_child_cursor: u32,
    /// Whether the most recently observed child was part of a text/
    /// interpolation run, so the next adjacent text/interpolation coalesces
    /// into the same DOM child rather than advancing the cursor.
    pub dom_in_text_run: bool,
    /// Queued child nav requests — this scope's own ref cannot be minted
    /// until every direct child has been visited (`PendingNavRequest`).
    ///
    /// [`PendingNavQueue`] is opaque here (default-construct and clear only).
    /// Construct/push/drain of `PendingNavRequest` stays in `vapor/**`.
    pub(in crate::template::code_gen) pending_nav_requests: PendingNavQueue,
}

impl Default for VaporElementState<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> VaporElementState<'a> {
    pub fn new() -> Self {
        Self {
            html: String::new(),
            text_parts: Vec::new(),
            node_ref: None,
            text_node_ref: None,
            own_effects: Vec::new(),
            child_nav: Vec::new(),
            child_text_creations: Vec::new(),
            child_effects: Vec::new(),
            child_statements: Vec::new(),
            named_slots: Vec::new(),
            dom_child_cursor: 0,
            dom_in_text_run: false,
            pending_nav_requests: PendingNavQueue::new(),
        }
    }

    /// Reset all fields while retaining allocated capacity.
    /// Used by the state pool to recycle instances.
    pub fn reset(&mut self) {
        self.html.clear();
        self.text_parts.clear();
        self.node_ref = None;
        self.text_node_ref = None;
        self.own_effects.clear();
        self.child_nav.clear();
        self.child_text_creations.clear();
        self.child_effects.clear();
        self.child_statements.clear();
        self.named_slots.clear();
        self.dom_child_cursor = 0;
        self.dom_in_text_run = false;
        self.pending_nav_requests.clear();
    }

    /// Observe an adjacent text or interpolation child while walking this
    /// element's children: adjacent text/interpolation nodes coalesce into a
    /// single DOM child, so only the first of a run advances the cursor.
    pub fn observe_dom_text_run(&mut self) {
        if !self.dom_in_text_run {
            self.dom_in_text_run = true;
            self.dom_child_cursor += 1;
        }
    }

    /// Observe a rendered comment child: it breaks any text run and counts as
    /// one DOM child. Callers invoke this only when comment rendering is on.
    pub fn observe_dom_comment(&mut self) {
        self.dom_in_text_run = false;
        self.dom_child_cursor += 1;
    }

    /// Observe an element child, returning its 0-based DOM index within this
    /// element and advancing the cursor past it. An element breaks any text
    /// run and counts as one DOM child.
    pub fn observe_dom_element(&mut self) -> u32 {
        let index = self.dom_child_cursor;
        self.dom_in_text_run = false;
        self.dom_child_cursor += 1;
        index
    }

    /// Ensure a node ref is allocated for this element.
    pub fn ensure_node_ref(&mut self, counters: &mut VaporCounters) -> u32 {
        if let Some(r) = self.node_ref {
            r
        } else {
            let r = counters.next_node();
            self.node_ref = Some(r);
            r
        }
    }

    /// Ensure a text node ref is allocated for this element.
    ///
    /// Official `context.reference()` has no separate `xN`/`nN` id space
    /// (`const n2 = t0()` + `const x2 = _txt(n2)`, rc.3). Reuses
    /// `ensure_node_ref`; `text_node_ref` only marks "also emit `xN = _txt(nN)`".
    pub fn ensure_text_ref(&mut self, counters: &mut VaporCounters) -> u32 {
        if let Some(r) = self.text_node_ref {
            r
        } else {
            let r = self.ensure_node_ref(counters);
            self.text_node_ref = Some(r);
            r
        }
    }
}

/// Completed root element data ready for assembly.
#[derive(Debug)]
pub struct VaporRootElement<'a> {
    /// Template HTML string (empty for components/slots).
    pub html: String,
    /// Template index (tN). None for components/slots.
    pub template_idx: Option<u32>,
    /// Node ref index (nN).
    pub node_ref: u32,
    /// Navigation instructions.
    pub nav: Vec<&'a str>,
    /// Text node creations.
    pub text_creations: Vec<&'a str>,
    /// Root's own text-node ref (`x{N}`), distinct from child-bubbled
    /// `text_creations`. Set when the root's entire dynamic text is its own:
    /// `const x{own_text_ref} = _txt(n{node_ref})`.
    pub own_text_ref: Option<u32>,
    /// All effects (own + child).
    pub effects: Vec<VaporEffect<'a>>,
    /// Non-effect statements with authored anchors (relative to statement start).
    /// A v-if/v-for branch body can embed an interpolation identifier.
    pub statements: Vec<(&'a str, &'a [SegmentAnchor])>,
    /// v-once flag: effects are emitted as direct statements (no `_renderEffect` wrapper).
    pub v_once: bool,
    /// v-memo deps expression: effects are wrapped in `_withMemo(deps, ...)`.
    pub v_memo_expr: Option<String>,
}

#[cfg(test)]
#[path = "types_tests.rs"]
mod tests;
