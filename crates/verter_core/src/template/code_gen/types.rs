//! Code generation output accumulator and internal types.
//!
//! All codegen operations are deferred into [`CodeGenOutput`] vecs.
//! Nothing is applied to the source until [`CodeGenOutput::apply_to()`] is called.

use oxc_allocator::Allocator;

use crate::code_transform::CodeTransform;

use super::shared::helpers::{
    BuiltinComponentFlags, SsrHelper, SsrHelperFlags, VaporHelper, VaporHelperFlags, VdomHelper,
    VdomHelperFlags,
};

// ======================== CodeGenOutput ========================

/// Accumulated code generation operations.
///
/// All operations are deferred — nothing is applied until [`apply_to()`](Self::apply_to).
/// This avoids passing `CodeTransform` through every trait method and enables
/// a single O(n+m) batch application at the end.
pub struct CodeGenOutput<'alloc> {
    /// Replace source ranges: (start, end, replacement).
    /// Applied via `ct.batch_overwrite()` in sorted order.
    pub overwrites: Vec<(u32, u32, &'alloc str)>,

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

    /// Allocator reference for bump-allocating generated strings.
    alloc: &'alloc Allocator,
}

impl<'alloc> CodeGenOutput<'alloc> {
    /// Create a new empty output accumulator.
    pub fn new(alloc: &'alloc Allocator) -> Self {
        Self {
            overwrites: Vec::with_capacity(16),
            prepends: Vec::with_capacity(16),
            mapped_prepends: Vec::new(),
            vdom_imports: VdomHelperFlags::empty(),
            vapor_imports: VaporHelperFlags::empty(),
            ssr_imports: SsrHelperFlags::empty(),
            builtin_imports: BuiltinComponentFlags::empty(),
            moves: Vec::new(),
            wrapped_moves: Vec::new(),
            alloc,
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

    /// Push a source-mapped prepend-left with bump-allocated content.
    /// The generated chunk maps back to `source_pos` in the source map.
    /// The source map token is placed at the start of the content (offset 0).
    #[inline]
    pub fn prepend_alloc_mapped(&mut self, pos: u32, source_pos: u32, content: &str) {
        let allocated = self.alloc.alloc_str(content);
        self.mapped_prepends.push((pos, source_pos, 0, allocated));
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

    /// Sort and apply all accumulated operations to a CodeTransform.
    /// Called once after the entire tree walk.
    ///
    /// Returns the categorized runtime helper imports collected during codegen.
    /// Vue helpers go to `vue`, SSR helpers go to `ssr` (from `vue/server-renderer`).
    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    pub fn apply_to(mut self, ct: &mut CodeTransform<'alloc>) -> TemplateImports {
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
            // Merge regular prepends (unmapped) and mapped prepends into a
            // unified vec for batch_prepend_left_with_source_map.
            type PrependItem<'b> = (u32, Option<(u32, u32)>, &'b str);
            let mut all_prepends: Vec<PrependItem<'_>> =
                Vec::with_capacity(self.prepends.len() + self.mapped_prepends.len());
            for &(pos, content) in &self.prepends {
                all_prepends.push((pos, None, content));
            }
            for &(pos, src_pos, content_offset, content) in &self.mapped_prepends {
                all_prepends.push((pos, Some((src_pos, content_offset)), content));
            }
            // Stable sort to preserve insertion order for same-position prepends.
            all_prepends.sort_by_key(|(pos, _, _)| *pos);
            ct.batch_prepend_left_with_source_map(&all_prepends);
        }

        // Apply deferred move operations (e.g., slot reordering)
        for &(start, end, target) in &self.moves {
            ct.move_slice(start, end, target);
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
    /// Condition prefix for v-if/v-else-if elements (e.g., `"(show) ? "`).
    /// Emitted by the parent's separator logic to ensure correct ordering
    /// relative to comma separators. `None` for non-conditional elements
    /// and v-else (which has no prefix — the `: ` comes from the previous
    /// branch's scope close).
    pub condition_prefix: Option<String>,
    /// Byte offset of the v-if/v-else-if expression value in source.
    /// Used to emit source-mapped condition prefixes so the LSP can map
    /// the ternary condition back to the directive expression.
    pub condition_expr_start: Option<u32>,
    /// Length of the binding prefix (e.g., `__props.` = 9, `_ctx.` = 5) within
    /// the resolved expression inside `condition_prefix`. Used to split the
    /// condition prefix into unmapped prefix + mapped identifier + unmapped suffix
    /// for accurate per-identifier source mapping.
    pub condition_binding_prefix_len: usize,
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
    /// Fully-static element(s) emitted as `_createStaticVNode()`.
    /// After consolidation, a single `StaticVNode` record may span multiple
    /// consecutive static siblings, emitting them as one HTML string.
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
    For { is_keyed: bool },
    /// Close a v-slot wrapper.
    #[allow(dead_code)]
    SlotWrapper,
}

// ======================== Vapor-specific types ========================

/// Counter allocator for Vapor variable names.
///
/// Each call to `next_*()` returns the current value and increments.
/// Variable naming: `n0`, `n1` (nodes), `x0` (text), `p0` (path), `t0` (template).
#[derive(Debug, Default)]
pub struct VaporCounters {
    /// Node reference counter (n0, n1, ...).
    pub n: u32,
    /// Text node reference counter (x0, x1, ...).
    pub x: u32,
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

    pub fn next_text(&mut self) -> u32 {
        let v = self.x;
        self.x += 1;
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
    /// Dynamic expression: `_toDisplayString(_ctx.msg)`.
    Dynamic(&'a str),
}

impl VaporTextPart<'_> {
    /// Format this part as a JS expression fragment.
    pub fn to_js(&self) -> &str {
        match self {
            VaporTextPart::Static(s) => s,
            VaporTextPart::Dynamic(s) => s,
        }
    }

    /// Returns true if this is a dynamic part.
    #[cfg(test)]
    pub fn is_dynamic(&self) -> bool {
        matches!(self, VaporTextPart::Dynamic(_))
    }
}

/// A reactive effect to emit in a `_renderEffect` block.
#[derive(Debug, Clone, PartialEq, Eq)]
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
/// Pushed onto the stack when entering an element, popped on leave.
/// Contains only genuine output buffers — metadata like tag name, is_root,
/// is_void, depth, child_index, and has_dynamic_text are derived from the
/// AST on-demand in the caller.
#[derive(Debug)]
pub struct VaporElementState<'a> {
    /// Accumulated static HTML for this element's subtree.
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
    /// Statements (non-effect, like event handlers).
    pub child_statements: Vec<&'a str>,
    /// Named slot closure entries built from `<template v-slot>` children.
    /// Each string is a complete slot entry (e.g., `header: () => { ... }`).
    pub named_slots: Vec<String>,
}

impl Default for VaporElementState<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl VaporElementState<'_> {
    pub fn new() -> Self {
        Self {
            html: String::with_capacity(128),
            text_parts: Vec::new(),
            node_ref: None,
            text_node_ref: None,
            own_effects: Vec::new(),
            child_nav: Vec::new(),
            child_text_creations: Vec::new(),
            child_effects: Vec::new(),
            child_statements: Vec::new(),
            named_slots: Vec::new(),
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
    pub fn ensure_text_ref(&mut self, counters: &mut VaporCounters) -> u32 {
        if let Some(r) = self.text_node_ref {
            r
        } else {
            let r = counters.next_text();
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
    /// All effects (own + child).
    pub effects: Vec<VaporEffect<'a>>,
    /// Non-effect statements.
    pub statements: Vec<&'a str>,
    /// v-once flag: effects are emitted as direct statements (no `_renderEffect` wrapper).
    pub v_once: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxc_allocator::Allocator;

    #[test]
    fn code_gen_output_overwrite_pushes_to_vec() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        out.overwrite(0, 5, "hello");
        assert_eq!(out.overwrites.len(), 1);
        assert_eq!(out.overwrites[0].0, 0);
        assert_eq!(out.overwrites[0].1, 5);
        assert_eq!(out.overwrites[0].2, "hello");
    }

    #[test]
    fn code_gen_output_prepend_static_pushes_to_vec() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        out.prepend_static(10, "_ctx.");
        assert_eq!(out.prepends.len(), 1);
        assert_eq!(out.prepends[0].0, 10);
        assert_eq!(out.prepends[0].1, "_ctx.");
    }

    #[test]
    fn code_gen_output_prepend_alloc_pushes_to_vec() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        out.prepend_alloc(5, "dynamic");
        assert_eq!(out.prepends.len(), 1);
        assert_eq!(out.prepends[0].0, 5);
        assert_eq!(out.prepends[0].1, "dynamic");
    }

    #[test]
    fn apply_to_sorts_overwrites_by_start() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        // Push in reverse order
        out.overwrite(10, 15, "b");
        out.overwrite(0, 5, "a");

        let mut ct = crate::code_transform::CodeTransform::new("0123456789ABCDE", &alloc);
        out.apply_to(&mut ct);
        let result = ct.build_string();
        assert_eq!(result, "a56789b");
    }

    #[test]
    fn apply_to_sorts_prepends_by_position() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        // Push in reverse order
        out.prepend_static(5, "Y");
        out.prepend_static(2, "X");

        let mut ct = crate::code_transform::CodeTransform::new("ABCDEFGH", &alloc);
        out.apply_to(&mut ct);
        let result = ct.build_string();
        assert_eq!(result, "ABXCDEYFGH");
    }

    /// @ai-generated - Regression test: prepends at the same position must
    /// preserve insertion order (stable sort). This matters when scope_close
    /// suffixes and sibling comma separators are both prepended at an
    /// element's end position. Without stable sort, the comma can appear
    /// before the scope_close, producing invalid JS like `, : _createCommentVNode`.
    #[test]
    fn apply_to_preserves_same_position_prepend_order() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);

        // Simulate the real compilation pattern: scope_close is pushed early
        // (during child's leave_element), then many other prepends are added
        // for other parts of the template, then the sibling comma is pushed
        // (during parent's add_children_separators).
        // The target position where both scope_close and comma land:
        let target = 50u32;

        // First batch: prepends BEFORE the scope_close (from earlier template processing)
        for i in 0..40u32 {
            out.prepend_static(i, "x");
        }
        // scope_close is pushed at target position
        out.prepend_static(target, "SCOPE_CLOSE");
        // Second batch: many more prepends from other template elements
        // (these go to positions AFTER target, interleaved)
        for i in 0..60u32 {
            out.prepend_static(target + 1 + i, "y");
        }
        // Sibling comma is pushed much later at the SAME target position
        out.prepend_static(target, "COMMA");

        let source = &"_".repeat(200);
        let mut ct = crate::code_transform::CodeTransform::new(source, &alloc);
        out.apply_to(&mut ct);
        let result = ct.build_string();

        // The two same-position prepends must appear in insertion order
        assert!(
            result.contains("SCOPE_CLOSECOMMA"),
            "Same-position prepends must preserve insertion order.\n\
             Expected 'SCOPE_CLOSECOMMA' but got:\n{}",
            result
        );
    }

    // ==================== Imports ====================

    #[test]
    fn add_vdom_import_sets_flag() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        out.add_vdom_import(VdomHelper::CreateElementVNode);
        assert!(out.vdom_imports().has(VdomHelper::CreateElementVNode));
    }

    #[test]
    fn add_vdom_import_deduplicates() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        out.add_vdom_import(VdomHelper::ToDisplayString);
        out.add_vdom_import(VdomHelper::CreateElementVNode);
        out.add_vdom_import(VdomHelper::ToDisplayString); // duplicate
        let imports = out.vdom_imports().to_imports();
        assert_eq!(imports.len(), 2);
    }

    #[test]
    fn apply_to_returns_vdom_imports() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        out.add_vdom_import(VdomHelper::CreateCommentVNode);
        out.add_vdom_import(VdomHelper::ToDisplayString);

        let mut ct = crate::code_transform::CodeTransform::new("hello", &alloc);
        let imports = out.apply_to(&mut ct);
        assert_eq!(imports.vue.len(), 2);
        assert!(imports.vue.contains(&"_createCommentVNode"));
        assert!(imports.vue.contains(&"_toDisplayString"));
        assert!(imports.ssr.is_empty());
    }

    #[test]
    fn apply_to_returns_vapor_imports() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        out.add_vapor_import(VaporHelper::Template);
        out.add_vapor_import(VaporHelper::RenderEffect);

        let mut ct = crate::code_transform::CodeTransform::new("hello", &alloc);
        let imports = out.apply_to(&mut ct);
        assert_eq!(imports.vue.len(), 2);
        assert!(imports.vue.contains(&"_template"));
        assert!(imports.vue.contains(&"_renderEffect"));
        assert!(imports.ssr.is_empty());
    }

    #[test]
    fn empty_output_returns_empty_imports() {
        let alloc = Allocator::default();
        let out = CodeGenOutput::new(&alloc);

        let mut ct = crate::code_transform::CodeTransform::new("hello", &alloc);
        let imports = out.apply_to(&mut ct);
        assert!(imports.is_empty());
    }

    // ==================== VaporCounters ====================

    #[test]
    fn vapor_counters_increment() {
        let mut c = VaporCounters::default();
        assert_eq!(c.next_node(), 0);
        assert_eq!(c.next_node(), 1);
        assert_eq!(c.next_text(), 0);
        assert_eq!(c.next_text(), 1);
        assert_eq!(c.next_path(), 0);
        assert_eq!(c.next_template(), 0);
        assert_eq!(c.next_template(), 1);
    }

    // ==================== VaporTextPart ====================

    #[test]
    fn vapor_text_part_static() {
        let part = VaporTextPart::Static("\"hello\"");
        assert_eq!(part.to_js(), "\"hello\"");
        assert!(!part.is_dynamic());
    }

    #[test]
    fn vapor_text_part_dynamic() {
        let part = VaporTextPart::Dynamic("_toDisplayString(_ctx.msg)");
        assert_eq!(part.to_js(), "_toDisplayString(_ctx.msg)");
        assert!(part.is_dynamic());
    }

    // ==================== VaporEffect ====================

    #[test]
    fn vapor_effect_set_text() {
        let effect = VaporEffect::SetText {
            text_ref: 0,
            parts: vec![
                VaporTextPart::Static("\"hello \""),
                VaporTextPart::Dynamic("_toDisplayString(_ctx.msg)"),
            ],
        };
        assert_eq!(
            effect.to_code(),
            "_setText(x0, \"hello \" + _toDisplayString(_ctx.msg))"
        );
    }

    #[test]
    fn vapor_effect_set_text_single_part() {
        let effect = VaporEffect::SetText {
            text_ref: 1,
            parts: vec![VaporTextPart::Dynamic("_toDisplayString(_ctx.count)")],
        };
        assert_eq!(
            effect.to_code(),
            "_setText(x1, _toDisplayString(_ctx.count))"
        );
    }

    #[test]
    fn vapor_effect_set_class() {
        let effect = VaporEffect::SetClass {
            node_ref: 0,
            expr: "_ctx.cls",
        };
        assert_eq!(effect.to_code(), "_setClass(n0, _ctx.cls)");
    }

    #[test]
    fn vapor_effect_set_style() {
        let effect = VaporEffect::SetStyle {
            node_ref: 2,
            expr: "_ctx.sty",
        };
        assert_eq!(effect.to_code(), "_setStyle(n2, _ctx.sty)");
    }

    #[test]
    fn vapor_effect_set_prop() {
        let effect = VaporEffect::SetProp {
            node_ref: 0,
            attr: "title",
            expr: "_ctx.title",
        };
        assert_eq!(effect.to_code(), "_setProp(n0, \"title\", _ctx.title)");
    }

    #[test]
    fn vapor_effect_set_attr() {
        let effect = VaporEffect::SetAttr {
            node_ref: 1,
            attr: "data-id",
            expr: "_ctx.id",
        };
        assert_eq!(effect.to_code(), "_setAttr(n1, \"data-id\", _ctx.id)");
    }

    #[test]
    fn vapor_effect_set_html() {
        let effect = VaporEffect::SetHtml {
            node_ref: 0,
            expr: "_ctx.rawHtml",
        };
        assert_eq!(effect.to_code(), "_setHtml(n0, _ctx.rawHtml)");
    }

    #[test]
    fn vapor_effect_set_html_with_resolved_ref() {
        let effect = VaporEffect::SetHtml {
            node_ref: 1,
            expr: "rawHtml.value",
        };
        assert_eq!(effect.to_code(), "_setHtml(n1, rawHtml.value)");
    }

    #[test]
    fn vapor_effect_set_dynamic_props() {
        let effect = VaporEffect::SetDynamicProps {
            node_ref: 0,
            expr: "_ctx.obj",
        };
        assert_eq!(effect.to_code(), "_setDynamicProps(n0, [_ctx.obj])");
    }

    #[test]
    fn vapor_effect_set_dynamic_props_with_resolved_ref() {
        let effect = VaporEffect::SetDynamicProps {
            node_ref: 2,
            expr: "obj.value",
        };
        assert_eq!(effect.to_code(), "_setDynamicProps(n2, [obj.value])");
    }

    // ==================== VaporElementState ====================

    #[test]
    fn vapor_element_state_new() {
        let state = VaporElementState::new();
        assert!(state.node_ref.is_none());
        assert!(state.text_node_ref.is_none());
        assert!(state.html.is_empty());
        assert!(state.text_parts.is_empty());
        assert!(state.own_effects.is_empty());
        assert!(state.child_nav.is_empty());
    }

    #[test]
    fn vapor_element_state_ensure_node_ref() {
        let mut counters = VaporCounters::default();
        let mut state = VaporElementState::new();
        let r1 = state.ensure_node_ref(&mut counters);
        assert_eq!(r1, 0);
        // Second call returns same ref
        let r2 = state.ensure_node_ref(&mut counters);
        assert_eq!(r2, 0);
        // Counter only incremented once
        assert_eq!(counters.n, 1);
    }

    #[test]
    fn vapor_element_state_ensure_text_ref() {
        let mut counters = VaporCounters::default();
        let mut state = VaporElementState::new();
        let r1 = state.ensure_text_ref(&mut counters);
        assert_eq!(r1, 0);
        let r2 = state.ensure_text_ref(&mut counters);
        assert_eq!(r2, 0);
        assert_eq!(counters.x, 1);
    }

    // ==================== Mapped Prepends ====================

    /// @ai-generated — prepend_alloc_mapped pushes to mapped_prepends vec
    #[test]
    fn prepend_alloc_mapped_pushes_to_vec() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        out.prepend_alloc_mapped(10, 20, "(show) ? ");
        assert_eq!(out.mapped_prepends.len(), 1);
        assert_eq!(out.mapped_prepends[0].0, 10); // insertion pos
        assert_eq!(out.mapped_prepends[0].1, 20); // source pos
        assert_eq!(out.mapped_prepends[0].2, 0); // content_offset
        assert_eq!(out.mapped_prepends[0].3, "(show) ? ");
    }

    /// @ai-generated — apply_to merges mapped and regular prepends correctly
    #[test]
    fn apply_to_merges_mapped_and_regular_prepends() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        // Regular prepend at position 5
        out.prepend_static(5, "_ctx.");
        // Mapped prepend at position 3
        out.prepend_alloc_mapped(3, 100, "(show) ? ");

        let source = "ABCDEFGHIJ";
        let mut ct = crate::code_transform::CodeTransform::new(source, &alloc);
        out.apply_to(&mut ct);
        let result = ct.build_string();
        // Position 3: "(show) ? " inserted, position 5: "_ctx." inserted
        assert_eq!(result, "ABC(show) ? DE_ctx.FGHIJ");
    }

    /// @ai-generated — apply_to with mapped prepends produces source-mapped tokens
    #[test]
    fn apply_to_mapped_prepend_produces_source_map_token() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        // Insert "(show) ? " at position 5, mapped to source position 20
        out.prepend_alloc_mapped(5, 20, "(show) ? ");

        let source = "0123456789ABCDEFGHIJKLMNOP";
        let mut ct = crate::code_transform::CodeTransform::new(source, &alloc);
        out.apply_to(&mut ct);

        let map =
            ct.generate_map(crate::code_transform::SourceMapOptions::new().with_source("test.vue"));
        let tokens: Vec<_> = map.get_tokens().collect();

        // Find token mapping to source col 20
        let mapped = tokens
            .iter()
            .find(|t| t.get_src_col() == 20 && t.get_source_id().is_some());
        assert!(
            mapped.is_some(),
            "should have source-mapped token at src col 20"
        );
    }
}
