//! Code generation output accumulator and internal types.
//!
//! Source edits are deferred into the [`EditRecorder`] a [`CodeGenOutput`]
//! holds. Nothing is applied to the source until
//! [`CodeGenOutput::apply_to()`] is called.

use oxc_allocator::Allocator;

use crate::code_transform::{
    CodeTransform, CodeTransformError, EditRecorder, GeneratedContentMarker, MappedGeneratedText,
    SegmentAnchor,
};

use super::vapor::PendingNavQueue;

use super::shared::helpers::{
    BuiltinComponentFlags, SsrHelper, SsrHelperFlags, VaporHelper, VaporHelperFlags, VdomHelper,
    VdomHelperFlags,
};

// ======================== CodeGenOutput ========================

/// Accumulated code generation operations: the source edits (held by
/// [`EditRecorder`], the transform owner's deferred-edit record) plus the Vue
/// facts the edits do not carry — runtime helper imports, the helper and
/// module preambles, and the script-export placement facts.
///
/// All operations are deferred — nothing is applied until [`apply_to()`](Self::apply_to).
/// This avoids passing `CodeTransform` through every trait method and enables
/// a single O(n+m) batch application at the end.
#[derive(Clone)]
pub struct CodeGenOutput<'alloc> {
    /// The deferred source edits.
    pub(crate) edits: EditRecorder<'alloc>,

    /// VDOM runtime helper imports (bitflags, O(1) dedup).
    vdom_imports: VdomHelperFlags,

    /// Vapor runtime helper imports (bitflags, O(1) dedup).
    vapor_imports: VaporHelperFlags,

    /// SSR runtime helper imports from `vue/server-renderer` (bitflags, O(1) dedup).
    ssr_imports: SsrHelperFlags,

    /// Vue built-in component imports (Suspense, Teleport, etc.).
    builtin_imports: BuiltinComponentFlags,

    /// Inline-mode hoisted-constant module preamble, recorded via
    /// [`set_module_preamble`](Self::set_module_preamble). Transferred to
    /// [`TemplateImports::module_preamble`] in [`apply_to`](Self::apply_to)
    /// for the orchestrator to apply with `ct.prepend(...)`.
    module_preamble: Option<&'alloc str>,

    /// Declared script-export-placement facts (see
    /// [`record_sfc_export_fact`](Self::record_sfc_export_fact)), resolved
    /// into markers in [`apply_to`](Self::apply_to).
    sfc_export_facts: Vec<SfcExportFact<'alloc>>,
}

impl<'alloc> CodeGenOutput<'alloc> {
    /// Create a mapped accumulator for code-generation fixtures.
    #[cfg(test)]
    pub fn new(alloc: &'alloc Allocator) -> Self {
        Self::with_mode(alloc, crate::emission::MapMode::Map)
    }

    /// Preserve the transform owner's map policy while recording its edits.
    pub(crate) fn with_mode(alloc: &'alloc Allocator, mode: crate::emission::MapMode) -> Self {
        Self {
            edits: EditRecorder::with_mode(alloc, mode),
            vdom_imports: VdomHelperFlags::empty(),
            vapor_imports: VaporHelperFlags::empty(),
            ssr_imports: SsrHelperFlags::empty(),
            builtin_imports: BuiltinComponentFlags::empty(),
            module_preamble: None,
            sfc_export_facts: Vec::new(),
        }
    }

    /// See [`EditRecorder::overwrite`].
    #[inline]
    pub fn overwrite(&mut self, start: u32, end: u32, content: &str) {
        self.edits.overwrite(start, end, content);
    }

    /// See [`EditRecorder::overwrite_alloc`].
    #[inline]
    pub fn overwrite_alloc(&mut self, start: u32, end: u32, content: &'alloc str) {
        self.edits.overwrite_alloc(start, end, content);
    }

    /// See [`EditRecorder::overwrite_segmented`].
    #[inline]
    pub fn overwrite_segmented(
        &mut self,
        start: u32,
        end: u32,
        content: &str,
        anchors: &[SegmentAnchor],
    ) {
        self.edits.overwrite_segmented(start, end, content, anchors);
    }

    /// See [`EditRecorder::prepend_static`].
    #[inline]
    pub fn prepend_static(&mut self, pos: u32, content: &'static str) {
        self.edits.prepend_static(pos, content);
    }

    /// See [`EditRecorder::prepend_alloc`].
    #[inline]
    pub fn prepend_alloc(&mut self, pos: u32, content: &str) {
        self.edits.prepend_alloc(pos, content);
    }

    /// See [`EditRecorder::overwrite_or_root_prefix`].
    pub fn overwrite_or_root_prefix(&mut self, start: u32, end: u32, content: &str) {
        self.edits.overwrite_or_root_prefix(start, end, content);
    }

    /// See [`EditRecorder::overwrite_or_root_prefix_alloc`].
    pub fn overwrite_or_root_prefix_alloc(&mut self, start: u32, end: u32, content: &'alloc str) {
        self.edits
            .overwrite_or_root_prefix_alloc(start, end, content);
    }

    /// See [`EditRecorder::overwrite_or_root_prefix_segmented`].
    pub fn overwrite_or_root_prefix_segmented(
        &mut self,
        start: u32,
        end: u32,
        content: &str,
        anchors: &[SegmentAnchor],
    ) {
        self.edits
            .overwrite_or_root_prefix_segmented(start, end, content, anchors);
    }

    /// See [`EditRecorder::overwrite_or_root_suffix`].
    pub fn overwrite_or_root_suffix(&mut self, start: u32, end: u32, content: &str) {
        self.edits.overwrite_or_root_suffix(start, end, content);
    }

    /// See [`EditRecorder::overwrite_or_root_suffix_alloc`].
    pub fn overwrite_or_root_suffix_alloc(&mut self, start: u32, end: u32, content: &'alloc str) {
        self.edits
            .overwrite_or_root_suffix_alloc(start, end, content);
    }

    /// Record a declared script-export-placement fact for `content` — a
    /// chunk ALREADY inserted via one of this call's siblings above (e.g.
    /// [`overwrite_or_root_suffix_alloc`](Self::overwrite_or_root_suffix_alloc)).
    /// `binding_local_ranges` are every `__sfc__`→`_sfc_main` rename target
    /// within `content`; `export_local_range` is the terminal default-export
    /// statement's own range within `content`, when this chunk contains one.
    /// Resolved into built-output positions in [`apply_to`](Self::apply_to),
    /// once every later edit (import hoisting, moves) has been queued —
    /// never rediscovered by scanning the built script for these strings.
    #[inline]
    pub(crate) fn record_sfc_export_fact(
        &mut self,
        content: &'alloc str,
        binding_local_ranges: Vec<std::ops::Range<u32>>,
        export_local_range: Option<std::ops::Range<u32>>,
    ) {
        self.sfc_export_facts.push(SfcExportFact {
            content,
            binding_local_ranges,
            export_local_range,
        });
    }

    /// Record module-scope preamble content (inline-mode hoisted constants)
    /// for the caller to apply via `ct.prepend(...)` — NOT a position-
    /// anchored `prepend_alloc`/`overwrite`. A position-anchored prepend at
    /// the same source offset as another producer's own position-anchored
    /// prepend (e.g. the script codegen's user-import hoist, both commonly
    /// anchored at offset 0) loses the ordering race whenever that other
    /// producer's `apply_to` batch already ran first: `batch_prepend_left_static`
    /// only interleaves new items with *positioned* chunks it walks past —
    /// once an earlier batch's same-position item is baked into an opaque
    /// `Chunk::Inserted`, a later batch's item at that position is appended
    /// after it, not before. `ct.prepend` sidesteps this entirely (it
    /// manipulates `ct`'s intro directly, unconditionally ahead of every
    /// chunk), the same escape hatch the helper-import line already uses.
    #[inline]
    pub fn set_module_preamble(&mut self, content: &str) {
        self.module_preamble = Some(self.edits.alloc_str(content));
    }

    /// See [`EditRecorder::add_sourcemap_location`].
    #[inline]
    pub fn add_sourcemap_location(&mut self, offset: u32) {
        self.edits.add_sourcemap_location(offset);
    }

    /// See [`EditRecorder::prepend_alloc_mapped`].
    #[inline]
    pub fn prepend_alloc_mapped(&mut self, pos: u32, source_pos: u32, content: &str) {
        self.edits.prepend_alloc_mapped(pos, source_pos, content);
    }

    /// See [`EditRecorder::prepend_alloc_mapped_with_offset`].
    #[cfg(test)]
    #[inline]
    pub fn prepend_alloc_mapped_with_offset(
        &mut self,
        pos: u32,
        source_pos: u32,
        content_offset: u32,
        content: &str,
    ) {
        self.edits
            .prepend_alloc_mapped_with_offset(pos, source_pos, content_offset, content);
    }

    /// See [`EditRecorder::alloc_fmt`].
    #[inline]
    pub(in crate::template::code_gen) fn alloc_fmt(
        &mut self,
        args: std::fmt::Arguments<'_>,
    ) -> &'alloc str {
        self.edits.alloc_fmt(args)
    }

    /// See [`EditRecorder::overwrite_fmt`].
    #[inline]
    pub fn overwrite_fmt(&mut self, start: u32, end: u32, args: std::fmt::Arguments<'_>) {
        self.edits.overwrite_fmt(start, end, args);
    }

    /// See [`EditRecorder::prepend_fmt`].
    #[inline]
    pub fn prepend_fmt(&mut self, pos: u32, args: std::fmt::Arguments<'_>) {
        self.edits.prepend_fmt(pos, args);
    }

    /// See [`EditRecorder::prepend_fmt_mapped`].
    #[allow(dead_code)]
    #[inline]
    pub fn prepend_fmt_mapped(&mut self, pos: u32, source_pos: u32, args: std::fmt::Arguments<'_>) {
        self.edits.prepend_fmt_mapped(pos, source_pos, args);
    }

    /// See [`EditRecorder::prepend_fmt_mapped_with_offset`].
    #[allow(dead_code)]
    #[inline]
    pub fn prepend_fmt_mapped_with_offset(
        &mut self,
        pos: u32,
        source_pos: u32,
        content_offset: u32,
        args: std::fmt::Arguments<'_>,
    ) {
        self.edits
            .prepend_fmt_mapped_with_offset(pos, source_pos, content_offset, args);
    }

    /// See [`EditRecorder::prepend_mapped_generated_text`].
    #[inline]
    pub fn prepend_mapped_generated_text(&mut self, pos: u32, mgt: &MappedGeneratedText) {
        self.edits.prepend_mapped_generated_text(pos, mgt);
    }

    /// See [`EditRecorder::relocate`].
    #[inline]
    pub fn relocate(&mut self, start: u32, end: u32, target: u32, prefix: &str, suffix: &str) {
        self.edits.relocate(start, end, target, prefix, suffix);
    }

    /// Allocate a string in the bump allocator.
    #[inline]
    pub fn alloc_str(&self, s: &str) -> &'alloc str {
        self.edits.alloc_str(s)
    }

    /// See [`EditRecorder::alloc_segment_anchors`].
    #[inline]
    pub fn alloc_segment_anchors(&self, anchors: &[SegmentAnchor]) -> &'alloc [SegmentAnchor] {
        self.edits.alloc_segment_anchors(anchors)
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

    /// The recorded mapped replacements, in record order.
    #[cfg(test)]
    pub(crate) fn overwrites(&self) -> Vec<(u32, u32, &'alloc str)> {
        self.edits
            .replacements_of(|kind| *kind == crate::code_transform::ReplacementKind::Mapped)
    }

    /// Current capacity of the reusable format-sink scratch buffer.
    #[cfg(test)]
    #[inline]
    pub fn scratch_capacity(&self) -> usize {
        self.edits.scratch_capacity()
    }

    /// Apply all accumulated edits to a CodeTransform. Called once after the
    /// entire tree walk.
    ///
    /// Returns the categorized runtime helper imports collected during codegen.
    /// Vue helpers go to `vue`, SSR helpers go to `ssr` (from `vue/server-renderer`).
    /// A refused edit is the transform's typed error; the transform is left
    /// as the refused edit found it.
    #[cfg_attr(feature = "semantic-observe", hotpath::measure)]
    pub fn apply_to(
        self,
        ct: &mut CodeTransform<'alloc, impl verter_audit::observe::ObserveExec>,
    ) -> Result<TemplateImports<'alloc>, CodeTransformError> {
        let Self {
            edits,
            vdom_imports,
            vapor_imports,
            ssr_imports,
            builtin_imports,
            module_preamble,
            sfc_export_facts,
        } = self;

        edits.apply(ct)?;

        // Return whichever mode's imports are non-empty (only one is ever active)
        let mut vue = if !vdom_imports.is_empty() {
            vdom_imports.to_imports()
        } else {
            vapor_imports.to_imports()
        };

        // Append built-in component imports (Suspense, Teleport, etc.)
        if !builtin_imports.is_empty() {
            vue.extend(builtin_imports.to_imports());
        }

        let ssr = ssr_imports.to_imports();

        // Resolve every declared script-export-placement fact into markers
        // NOW — after every edit has been applied to `ct` — never earlier,
        // since a later edit can still shift where `content` lands.
        // `generated_content_range` (called by this method's own caller, once
        // `ct.build_string()` has run) turns each marker into its final
        // built-output byte range.
        let mut sfc_binding_markers = Vec::new();
        let mut sfc_export_statement_marker = None;
        for fact in &sfc_export_facts {
            for range in &fact.binding_local_ranges {
                if let Some(marker) =
                    ct.mark_generated_sub_range(fact.content, range.start, range.end)
                {
                    sfc_binding_markers.push(marker);
                }
            }
            if let Some(range) = &fact.export_local_range {
                if let Some(marker) =
                    ct.mark_generated_sub_range(fact.content, range.start, range.end)
                {
                    sfc_export_statement_marker = Some(marker);
                }
            }
        }

        Ok(TemplateImports {
            vue,
            ssr,
            sfc_binding_markers,
            sfc_export_statement_marker,
            module_preamble,
        })
    }
}

/// One producer's declared script-export-placement fact: `content` is a
/// chunk already queued for insertion elsewhere in this `CodeGenOutput`;
/// `binding_local_ranges`/`export_local_range` are byte ranges WITHIN
/// `content`'s own LOCAL coordinate space — never a range from a different
/// content chunk or a different space (assembled-output, original-source).
#[derive(Clone)]
struct SfcExportFact<'alloc> {
    content: &'alloc str,
    binding_local_ranges: Vec<std::ops::Range<u32>>,
    export_local_range: Option<std::ops::Range<u32>>,
}

// ======================== TemplateImports ========================

/// Categorized runtime helper imports from template codegen.
///
/// Separates Vue helpers (from `"vue"`) and SSR helpers (from `"vue/server-renderer"`)
/// so the caller can emit two distinct import lines for SSR builds.
pub struct TemplateImports<'alloc> {
    /// Helpers imported from `"vue"` (e.g., `_mergeProps`, `_resolveComponent`).
    pub vue: Vec<&'static str>,
    /// Helpers imported from `"vue/server-renderer"` (e.g., `_ssrRenderAttrs`).
    pub ssr: Vec<&'static str>,
    /// Every declared `__sfc__`→`_sfc_main` rename target, as an unresolved
    /// marker — resolve with `ct.generated_content_range(marker)` once
    /// `ct.build_string()` has run (every later edit must be queued first).
    /// Empty for every template-codegen caller; populated only by script
    /// codegen's `CodeGenOutput::record_sfc_export_fact` producers.
    pub(crate) sfc_binding_markers: Vec<GeneratedContentMarker<'alloc>>,
    /// The declared terminal default-export statement's own range, as an
    /// unresolved marker. `None` when no script producer recorded one.
    pub(crate) sfc_export_statement_marker: Option<GeneratedContentMarker<'alloc>>,
    /// Inline-mode hoisted-constant preamble (`const _hoisted_1 = [...]\n`),
    /// when template codegen recorded one via
    /// [`CodeGenOutput::set_module_preamble`]. The caller must apply it with
    /// `ct.prepend(...)` — NOT a position-anchored `overwrite`/`prepend_alloc`
    /// — see that method's doc comment for why. `None` when no hoisted
    /// preamble was emitted (non-inline mode, or no hoisted constants).
    pub module_preamble: Option<&'alloc str>,
}

impl TemplateImports<'_> {
    /// Returns true if there are no imports at all.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.vue.is_empty() && self.ssr.is_empty()
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
/// (rc.5). No `next_text()`; callers use `next_node()` (`ensure_text_ref`).
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
    /// `_setText(xN, parts...)` when `generated` (extracted via `_txt()`),
    /// or `_setText(nN, parts...)` when the ref is already a plain nav-chain
    /// node reached through shared `_child`/`_next` (a mixed-children
    /// container's text run — official `SetTextIRNode.generated`).
    SetText {
        text_ref: u32,
        parts: Vec<VaporTextPart<'a>>,
        generated: bool,
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
    /// `_setDOMProp(nN, "attr", expr)` — an explicit `.prop`-modified binding
    /// forced to the DOM-property setter regardless of hyphenation.
    SetDomProp {
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
            VaporEffect::SetText {
                text_ref,
                parts,
                generated,
            } => {
                buf.push_str("_setText(");
                buf.push(if *generated { 'x' } else { 'n' });
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
            VaporEffect::SetText {
                text_ref,
                parts,
                generated,
            } => {
                buf.push_str("_setText(");
                buf.push(if *generated { 'x' } else { 'n' });
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
                            ..*a
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
            VaporEffect::SetDomProp {
                node_ref,
                attr,
                expr,
            } => {
                buf.push_str("_setDOMProp(n");
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

/// What kind of construct a merge into a parent's `element_stack` entry
/// brought in — official's `markSlotRootOperations` branches on this to
/// decide slot-root forwarding. Only `SlotOutlet` and `DynamicComponent` are
/// ever NON_STABLE-forwarding-eligible (a statically-resolved component or a
/// v-if/v-for chain is `Other` — excluded, matching `hasStableSlotRoot`/
/// `markSlotRootComponent`'s `!isStatic` gate).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergedConstructKind {
    SlotOutlet,
    DynamicComponent,
    Other,
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
    /// Whether `text_node_ref` needs a `_txt()` extraction statement
    /// (`xN`) or is already a plain nav-chain node reference reached
    /// through shared `_child`/`_next` (`nN`) — set `false` only for a
    /// text run that is a direct child of a mixed-content container
    /// (`children_all_text_like == false`), mirrors official
    /// `SetTextIRNode.generated`. Meaningless when `text_node_ref` is
    /// `None`; defaults `true` (the pre-existing `_txt()` shape).
    pub text_ref_generated: bool,
    /// Set at `enter_element` by scanning this element's own children: `true`
    /// unless at least one meaningful sibling is STRUCTURAL (component,
    /// slot outlet, `<template v-slot>`, or a v-if/v-for-wrapped element —
    /// `!is_template_contributing`). A plain element/comment sibling leaves
    /// this `true` (narrower than official `isAllTextLike`, which any
    /// element/comment sibling flips — Verter's existing deferred/combined
    /// effect ordering already matches official there; only the structural
    /// case has a real flush-timing requirement — see `enter_element`'s own
    /// comment). A direct interpolation child only reuses
    /// this element's own ref for `_txt()` extraction when `true`; when
    /// `false` it instead reserves a shared nav-chain slot.
    pub children_all_text_like: bool,
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
    /// The scope HTML buffer's length at the start of the CURRENT DOM text
    /// run (recorded when `dom_in_text_run` transitions `false` → `true`).
    /// `None` outside a run. Lets the walker collapse a run's static prefix
    /// back to a single space placeholder the moment an interpolation
    /// arrives — see `text_run_has_dynamic` and
    /// `VaporCodeGen::visit_text`/`visit_interpolation`.
    pub text_run_html_start: Option<usize>,
    /// Whether the CURRENT DOM text run has been established as dynamic
    /// (contains at least one interpolation seen so far). Official Vue
    /// collapses an entire dynamic run to ONE space in the static HTML
    /// template, with all its content (static and dynamic) living only in
    /// the `_setText`/`_toDisplayString` runtime expression — never baked
    /// into the hoisted template string.
    pub text_run_has_dynamic: bool,
    /// Queued child nav requests — this scope's own ref cannot be minted
    /// until every direct child has been visited (`PendingNavRequest`).
    ///
    /// [`PendingNavQueue`] is opaque here (default-construct and clear only).
    /// Construct/push/drain of `PendingNavRequest` stays in `vapor/**`.
    pub(in crate::template::code_gen) pending_nav_requests: PendingNavQueue,
    /// Set at `enter_element` from this element's OWN tag kind: `true` for a
    /// component (including a dynamic `<component :is>`/`KeepAlive`) or a
    /// `<template v-slot>` wrapper — a scope with no real DOM container of
    /// its own, whose children become slot content rather than DOM
    /// descendants. Read by a CHILD's merge (`self.element_stack.last()` is
    /// the parent at that point) to decide slot-root flag eligibility.
    pub is_component_scope: bool,
    /// Set at `enter_element` for a `<template v-if>`/`<template v-else-if>`/
    /// `<template v-else>`/`<template v-for>` (no `v-slot`) — official Vue
    /// treats every `<template>` tag as transparent IR, never a real DOM
    /// node: it builds no open tag and owns no template/container of its
    /// own. Distinct from `is_component_scope` (that flag also drives
    /// slot-root flag eligibility, which a transparent template does NOT
    /// participate in — confirmed against the pinned oracle: a component
    /// nested directly under `<template v-if>` gets no `SLOT_ROOT` flag).
    /// Read by `merge_into_stack_index` to decide whether a merged
    /// structural child (v-if/v-for/component/slot) may be donated directly
    /// as this scope's own body instead of nav/insertion-state-merged into
    /// a container that doesn't exist.
    pub is_transparent_wrapper: bool,
    /// Set when a structural child was donated directly into this scope
    /// (see `is_transparent_wrapper`): `node_ref` and `child_statements`
    /// already fully express this scope's body (the donated construct's own
    /// statement, which declares and returns its ref) — `build_closure_body`
    /// must not additionally register a template/emit a `tN()` instantiation.
    pub donated_construct: bool,
    /// `(kind, own node ref)` of each child merged into this scope via
    /// `merge_into_stack_index`, in visit order — used to detect the narrow
    /// "this scope's entire content is one non-stable construct" case that
    /// forwards through `_extend(..., { _: 1 /* NON_STABLE */ })` instead of
    /// the normal slot form (official `hasStableSlotRoot`/
    /// `markSlotRootOperations`). The node ref is the merged construct's own
    /// root ref — the `return nN` target when that path applies.
    pub merged_construct_kinds: Vec<(MergedConstructKind, u32)>,
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
            text_ref_generated: true,
            children_all_text_like: true,
            own_effects: Vec::new(),
            child_nav: Vec::new(),
            child_text_creations: Vec::new(),
            child_effects: Vec::new(),
            child_statements: Vec::new(),
            named_slots: Vec::new(),
            dom_child_cursor: 0,
            dom_in_text_run: false,
            text_run_html_start: None,
            text_run_has_dynamic: false,
            pending_nav_requests: PendingNavQueue::new(),
            is_component_scope: false,
            is_transparent_wrapper: false,
            donated_construct: false,
            merged_construct_kinds: Vec::new(),
        }
    }

    /// Reset all fields while retaining allocated capacity.
    /// Used by the state pool to recycle instances.
    pub fn reset(&mut self) {
        self.html.clear();
        self.text_parts.clear();
        self.node_ref = None;
        self.text_node_ref = None;
        self.text_ref_generated = true;
        self.children_all_text_like = true;
        self.own_effects.clear();
        self.child_nav.clear();
        self.child_text_creations.clear();
        self.child_effects.clear();
        self.child_statements.clear();
        self.named_slots.clear();
        self.dom_child_cursor = 0;
        self.dom_in_text_run = false;
        self.text_run_html_start = None;
        self.text_run_has_dynamic = false;
        self.pending_nav_requests.clear();
        self.is_component_scope = false;
        self.is_transparent_wrapper = false;
        self.donated_construct = false;
        self.merged_construct_kinds.clear();
    }

    /// Observe an adjacent text or interpolation child while walking this
    /// element's children: adjacent text/interpolation nodes coalesce into a
    /// single DOM child, so only the first of a run advances the cursor.
    /// Returns `true` when this call started a NEW run (the caller then
    /// records `text_run_html_start`/resets `text_run_has_dynamic` itself,
    /// since only it holds the live `html` buffer to measure).
    pub fn observe_dom_text_run(&mut self) -> bool {
        if !self.dom_in_text_run {
            self.dom_in_text_run = true;
            self.dom_child_cursor += 1;
            true
        } else {
            false
        }
    }

    /// Observe a rendered comment child: it breaks any text run and counts as
    /// one DOM child. Callers invoke this only when comment rendering is on.
    pub fn observe_dom_comment(&mut self) {
        self.dom_in_text_run = false;
        self.text_run_html_start = None;
        self.text_run_has_dynamic = false;
        self.dom_child_cursor += 1;
    }

    /// Observe an element child, returning its 0-based DOM index within this
    /// element and advancing the cursor past it. An element breaks any text
    /// run and counts as one DOM child.
    pub fn observe_dom_element(&mut self) -> u32 {
        let index = self.dom_child_cursor;
        self.dom_in_text_run = false;
        self.text_run_html_start = None;
        self.text_run_has_dynamic = false;
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
    /// (`const n2 = t0()` + `const x2 = _txt(n2)`, rc.5). Reuses
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

    /// Reserve a nav-chain text ref for a direct text/interpolation child of
    /// a MIXED-content container (`children_all_text_like == false`) —
    /// official `processInterpolation`'s `context.reference()`, a fresh id
    /// distinct from this element's own container ref (unlike
    /// `ensure_text_ref`, which deliberately reuses it). Idempotent per text
    /// run: `Some(id)` only on the first call (the caller then queues the
    /// nav-chain reservation); later calls in the same run return `None`.
    pub fn reserve_nav_text_ref(&mut self, counters: &mut VaporCounters) -> Option<u32> {
        if self.text_node_ref.is_some() {
            return None;
        }
        let r = counters.next_node();
        self.text_node_ref = Some(r);
        self.text_ref_generated = false;
        Some(r)
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
    /// Root's own text-node ref, distinct from child-bubbled
    /// `text_creations`. Set when the root's entire dynamic text is its own.
    /// When `own_text_ref_generated`, needs its own extraction statement:
    /// `const x{own_text_ref} = _txt(n{node_ref})`; otherwise the ref is
    /// already a nav-chain node established in `nav` — no extraction line.
    pub own_text_ref: Option<u32>,
    /// See `VaporElementState::text_ref_generated`. Meaningless when
    /// `own_text_ref` is `None`.
    pub own_text_ref_generated: bool,
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
