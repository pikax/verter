//! The state of one virtual-DOM render run: the recorded edits, the helper
//! flags, the per-run options, and the planning tables the walk fills.

use std::ops::{Deref, DerefMut};

use rustc_hash::FxHashMap;

use crate::ast::types::{
    AstNodeKind, ElementNodeCondition, ElementNodeConditionKind, TagType, TemplateAst,
};
use crate::code_transform::{EditRecorder, MappedGeneratedText, SegmentAnchor};
use crate::framework::vue::semantic::VueBindingView;
use crate::framework::vue::structure::{VueChildKind, VueCompileNodeKey};
use crate::target_plan::SparseOverlay;
use crate::template::code_gen::shared::helpers::{
    self, BuiltinComponentFlags, VdomHelper, VdomHelperFlags,
};

use super::plan::VdomNodePlan;
use crate::framework::vue::target::VueTargetInputs;
use crate::template::code_gen::types::{ChildKind, ChildRecord, ConditionChainRole, ScopeClose};
use crate::template::topology::{VueTemplateNodeId, VueTemplateNodeKind};
use crate::types::NodeId;
use verter_span::Span;

/// Where the source of a dropped child is removed before the walk.
#[derive(Clone, Copy)]
enum DroppedRemoval {
    /// A wider edit made when the parent is left covers it.
    Covered,
    /// Every dropped child.
    All,
    /// Dropped children between these offsets (the root's first and last
    /// kept child); the render prefix and suffix cover the edges.
    Between(u32, u32),
}

/// The options of one run, resolved once from the target options and the
/// component's scope plan.
#[derive(Debug, Clone)]
pub(crate) struct VdomOptions {
    pub is_inline: bool,
    pub is_production: bool,
    pub comments: bool,
    pub force_js: bool,
    pub has_script: bool,
    pub hoist_static: bool,
    /// Inline handlers are held in `_cache`. When they are not, a listener
    /// is a dynamic prop like any other binding.
    pub cache_handlers: bool,
    /// Plain elements are created with `_createElementVNode`. Content written
    /// into a server render function's client branch uses `_createVNode`,
    /// as the official server compiler does.
    pub element_vnodes: bool,
    /// Separator between the items of a child array. Content written into a
    /// server render function puts each item on its own line.
    pub array_separator: &'static str,
    /// The branches of a `v-if` chain each start a line (`(a)\n  ? …\n  : …`),
    /// as content written into a server render function lays them out.
    pub branches_on_lines: bool,
    /// Interpolations drop the spaces inside their delimiters, as the
    /// official compiler writes them.
    pub trim_interpolations: bool,
    /// The scope attribute (`data-v-<hash>`) when a scoped style exists,
    /// else empty.
    pub scope_id: String,
}

impl VdomOptions {
    /// The block helper of an element or fragment: `_createElementBlock`,
    /// or `_createBlock` in content written into a server render function.
    pub(super) fn element_block(&self) -> VdomHelper {
        if self.element_vnodes {
            VdomHelper::CreateElementBlock
        } else {
            VdomHelper::CreateBlock
        }
    }

    pub(super) fn of(inputs: &VueTargetInputs<'_>) -> Self {
        let options = inputs.options;
        Self {
            is_inline: options.inline,
            is_production: options.is_production,
            comments: options.comments,
            force_js: options.force_js,
            has_script: options.has_script,
            hoist_static: options.hoist_static,
            cache_handlers: true,
            element_vnodes: true,
            array_separator: ", ",
            trim_interpolations: false,
            branches_on_lines: false,
            scope_id: if inputs.scope.has_scoped {
                inputs.scope.scope.attribute()
            } else {
                String::new()
            },
        }
    }
}

/// The edits and runtime helpers one render run records. Edit methods are
/// the recorder's own, reached through `Deref`.
pub(crate) struct VdomOut<'c> {
    pub(crate) edits: EditRecorder<'c>,
    vdom_imports: VdomHelperFlags,
    builtin_imports: BuiltinComponentFlags,
}

impl<'c> VdomOut<'c> {
    pub(crate) fn new(edits: EditRecorder<'c>) -> Self {
        Self {
            edits,
            vdom_imports: VdomHelperFlags(0),
            builtin_imports: BuiltinComponentFlags(0),
        }
    }

    /// Record a runtime helper import.
    #[inline]
    pub(crate) fn add_vdom_import(&mut self, h: VdomHelper) {
        self.vdom_imports = self.vdom_imports.add(h);
    }

    /// Record a framework built-in component import (`Suspense`, `Teleport`, …).
    #[inline]
    pub(crate) fn add_builtin_component(&mut self, flag: u8) {
        self.builtin_imports = self.builtin_imports.add(flag);
    }

    #[cfg(test)]
    pub(crate) fn vdom_imports(&self) -> VdomHelperFlags {
        self.vdom_imports
    }

    /// The recorded mapped replacements, in record order.
    #[cfg(test)]
    pub(crate) fn overwrites(&self) -> Vec<(u32, u32, &'c str)> {
        self.edits
            .replacements_of(|kind| *kind == crate::code_transform::ReplacementKind::Mapped)
    }

    /// The edits, and the runtime helpers in import order followed by the
    /// built-in components.
    fn into_parts(self) -> (EditRecorder<'c>, Vec<&'static str>) {
        let mut runtime = self.vdom_imports.to_imports();
        runtime.extend(self.builtin_imports.to_imports());
        (self.edits, runtime)
    }
}

impl<'c> Deref for VdomOut<'c> {
    type Target = EditRecorder<'c>;
    fn deref(&self) -> &Self::Target {
        &self.edits
    }
}

impl DerefMut for VdomOut<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.edits
    }
}

/// Virtual-DOM render state: what one run of the target holds while it
/// walks the template.
///
/// Child records come from the compile structure's resolved children, so
/// whitespace and branch-gap decisions are never made here.
pub struct VdomState<'c> {
    /// Every shared prerequisite of the run.
    pub(super) inputs: &'c VueTargetInputs<'c>,
    /// The SFC source every offset indexes.
    pub(super) source: &'c str,
    /// The recorded edits and helper flags. Taken out while a body that
    /// receives it as its own parameter runs, and put back after.
    pub(super) out: Option<VdomOut<'c>>,
    /// Topology nodes the compile structure drops (whitespace the policy
    /// removes, nodes between branches of one chain, disabled comments),
    /// by topology index. Their source is removed in `enter_template`.
    pub(super) dropped: Vec<bool>,
    /// Reference to the template AST arena for O(1) node lookups.
    pub(super) ast: &'c TemplateAst,
    /// NodeId-aligned OXC parse data — used for the official-parity
    /// `hasScopeRef` slot-flag decision (scanning a component's slot
    /// subtree for references to outer template-scope variables).
    pub(super) oxc_ast: &'c crate::template::oxc::types::OxcParsedAst<'c>,
    pub(super) resolver: VueBindingView<'c>,
    pub(super) options: VdomOptions,
    /// Reusable buffer for building open/close tag strings.
    /// Uses `std::mem::take` pattern to avoid per-element allocation.
    pub(super) buf: String,
    /// Scope close stack for structural directives (v-if, v-for).
    /// Pushed in `enter_element`, popped in `leave_element`.
    pub(super) scope_closes: Vec<Option<ScopeClose>>,
    /// v-for prefix stack. Stored during `enter_element` and consumed by
    /// `process_element_leave` to include in the open tag overwrite. This
    /// ensures correct ordering when a sibling text node ends at the same
    /// position as the v-for element starts.
    /// Tuple: (prefix_string, iterable_source_start) for source map mapping.
    pub(super) v_for_prefixes: Vec<Option<(String, Option<u32>)>>,
    /// Pre-computed condition expressions with binding resolution, carried as
    /// segment plans so the ternary head maps authored identifiers to source
    /// while leaving synthetic scaffolding unmapped.
    /// Populated during `enter_element` (where OXC data is available) and
    /// consumed by `build_child_records` (which only sees AST data).
    /// Keyed by AST node index. Holds the bare resolved expression (no `(` …
    /// `) ? ` wrapper); `build_child_records` wraps it per element.
    pub(super) resolved_condition_prefixes: FxHashMap<usize, MappedGeneratedText>,
    /// Whether the template has a single effective root element (not multi-root).
    /// Set in `enter_template`, used by `leave_element` to determine if a root
    /// element should be a block root (`_createElementBlock` / `_createBlock`).
    pub(super) single_root: bool,
    /// Hoisted constant strings (e.g., `["id"]`) collected during codegen,
    /// each paired with its OWN authored anchors (relative to the string's
    /// own start) — a hoisted static-props object can embed a static
    /// attribute's own key (e.g. `class`), and that anchor must survive
    /// into the hoisted-preamble insertion (see
    /// `code_transform::segmented`'s module doc). Empty when the string
    /// carries no mappable key (the common case — most hoisted constants
    /// are `["id"]`-style arrays or `{ key: N }` branch markers). Emitted as
    /// `const _hoisted_N = ...` before the render function. Deduplicated:
    /// identical strings share the same `_hoisted_N` reference (first
    /// occurrence's anchors win — see `reserve_hoist`).
    pub(super) hoisted_constants: Vec<(String, Vec<SegmentAnchor>)>,
    /// Cache index counter for `_cache[N]` static element wrapping.
    /// Incremented each time a fully-static element is cached.
    pub(super) cache_index: usize,
    /// Whether we are currently inside a slot function body.
    /// When true, `leave_element` skips individual `_cache[N]` wrapping
    /// because slot-level cache grouping handles it instead.
    /// Stored as a stack to handle nested slot contexts.
    pub(super) in_slot_context_stack: Vec<bool>,
    /// Hoisted _resolveComponent() calls: Vec of (tag_name, variable_name).
    /// Emitted as `const _component_x = _resolveComponent("x")` at the top
    /// of the render function body. Insertion-ordered.
    pub(super) resolved_components: Vec<super::component::ResolvedComponent>,
    /// Per-item v-memo close suffix for a `v-for` + `v-memo` element, keyed by
    /// AST node index. Built in `enter_element` (where the v-for prefix and memo
    /// index are computed) and applied in `leave_element` in place of the normal
    /// `_renderList` fragment close.
    pub(super) memo_for_suffixes: FxHashMap<usize, String>,
    /// What the planning passes reserved per element: its own hoisted
    /// constant, its cached handlers, its static children array group, and
    /// whether its parent caches it as part of that group. See
    /// [`VdomNodePlan`].
    pub(super) plans: SparseOverlay<VueCompileNodeKey, VdomNodePlan>,
    /// The module-level declarations the render function needs
    /// (`const _hoisted_N = …`), with their own authored anchors; set by
    /// `leave_template` and emitted as the preamble plan.
    pub(super) module_preamble: Option<(String, Vec<SegmentAnchor>)>,
    /// Whether constants may be hoisted to module scope. Off for slot
    /// content written into a server render function, which has no
    /// preamble.
    pub(super) hoisting: bool,
}

impl<'c> VdomState<'c> {
    pub(super) fn new(inputs: &'c VueTargetInputs<'c>, resolver: VueBindingView<'c>) -> Self {
        let edits = EditRecorder::with_mode(inputs.allocator, inputs.options.map_mode);
        Self {
            inputs,
            source: inputs.source.text,
            out: Some(VdomOut::new(edits)),
            dropped: Vec::new(),
            ast: inputs.topology.ast(),
            oxc_ast: inputs.semantic.expressions(),
            resolver,
            options: VdomOptions::of(inputs),
            buf: String::with_capacity(128),
            scope_closes: Vec::new(),
            v_for_prefixes: Vec::new(),
            resolved_condition_prefixes: FxHashMap::default(),
            single_root: false,
            hoisted_constants: Vec::new(),
            cache_index: 0,
            in_slot_context_stack: Vec::new(),
            resolved_components: Vec::new(),
            memo_for_suffixes: FxHashMap::default(),
            plans: SparseOverlay::new(),
            module_preamble: None,
            hoisting: true,
        }
    }

    /// The recorded output, taken while a body that receives it as its own
    /// parameter runs. Every taker puts it back with [`Self::restore_out`].
    pub(super) fn take_out(&mut self) -> VdomOut<'c> {
        self.out
            .take()
            .expect("the render output is held by one body at a time")
    }

    pub(super) fn restore_out(&mut self, out: VdomOut<'c>) {
        self.out = Some(out);
    }

    /// The recorded output once the walk is done: the edits, and the runtime
    /// helpers in import order followed by the built-in components. `_unref`
    /// is imported when the binding view read any identifier through it.
    pub(super) fn finish_out(
        &mut self,
        mut out: VdomOut<'c>,
    ) -> (EditRecorder<'c>, Vec<&'static str>) {
        if self.resolver.uses_unref() {
            out.add_vdom_import(VdomHelper::Unref);
        }
        out.into_parts()
    }

    /// Mark every topology node the compile structure drops, and render a
    /// kept whitespace child as one space. Runs once, before the walk, so no
    /// visit decides whether its node survives.
    ///
    /// The source of a dropped node is removed here only where no wider edit
    /// covers it: a dropped comment always; whitespace and branch gaps
    /// between the root's first and last kept child; every dropped child of
    /// a `<template v-if>` / `<template v-for>` fragment. Inside elements
    /// and slots, tag extension and gap filling remove it when the element
    /// is left; at the root's edges the render prefix and suffix cover it.
    ///
    /// With `content`, only slot content is written: the owner's children
    /// inside the content window, whose dropped children are all removed,
    /// and nothing outside the window is touched.
    pub(super) fn settle_children(
        &mut self,
        out: &mut VdomOut<'c>,
        content: Option<(VueTemplateNodeId, Span)>,
    ) {
        let topology = self.inputs.topology;
        let inside = |span: Span| {
            content.is_none_or(|(_, window)| span.start >= window.start && span.end <= window.end)
        };
        let structure = self.inputs.structure;
        let mut kept = vec![false; topology.len()];
        let mut dropped = vec![false; topology.len()];
        for parent in topology.nodes() {
            let resolved = structure.children(structure.compile_id(parent));
            for child in resolved {
                let node = structure.template_id(child.node);
                kept[node.index()] = true;
                if matches!(
                    child.kind,
                    VueChildKind::WhitespaceNewline | VueChildKind::WhitespaceSpace
                ) {
                    let span = topology.span(node);
                    if inside(span) {
                        out.overwrite(span.start, span.end, " ");
                    }
                }
            }
            let removal = if content.is_some_and(|(owner, _)| owner == parent) {
                DroppedRemoval::All
            } else if parent == topology.root() {
                let first = resolved
                    .first()
                    .map(|c| topology.span(structure.template_id(c.node)));
                let last = resolved
                    .last()
                    .map(|c| topology.span(structure.template_id(c.node)));
                match (first, last) {
                    (Some(first), Some(last)) => DroppedRemoval::Between(first.end, last.start),
                    _ => DroppedRemoval::Covered,
                }
            } else if topology.element(parent).is_some_and(|el| {
                el.tag_type == TagType::Template
                    && el.v_slot.is_none()
                    && (el.v_condition.is_some() || el.v_for.is_some())
            }) {
                DroppedRemoval::All
            } else {
                DroppedRemoval::Covered
            };
            for node in topology.children(parent) {
                if kept[node.index()] {
                    continue;
                }
                verter_debug_assert!(
                    topology.element(node).is_none(),
                    "the compile structure drops only text and comments"
                );
                dropped[node.index()] = true;
                let span = topology.span(node);
                let remove = match removal {
                    _ if topology.kind(node) == VueTemplateNodeKind::Comment => true,
                    DroppedRemoval::All => true,
                    DroppedRemoval::Between(start, end) => span.start >= start && span.end <= end,
                    DroppedRemoval::Covered => false,
                };
                if remove && span.start < span.end && inside(span) {
                    out.overwrite(span.start, span.end, "");
                }
            }
        }
        self.dropped = dropped;
    }

    /// What follows a branch condition: `) ? `, or a new line.
    fn branch_question(&self) -> &'static str {
        if self.options.branches_on_lines {
            ")\n  ? "
        } else {
            ") ? "
        }
    }

    /// Write the close of a structural directive at `pos`. On lines, the
    /// ternary's ` : ` starts a new line.
    pub(super) fn prepend_scope_close(&self, out: &mut VdomOut<'c>, pos: u32, close: &ScopeClose) {
        let text = super::directives::format_scope_close(close, self.options.is_production);
        if text.is_empty() {
            return;
        }
        match text.find(" : ") {
            Some(at) if self.options.branches_on_lines => {
                out.prepend_alloc(pos, &[&text[..at], "\n  : ", &text[at + 3..]].concat());
            }
            _ => out.prepend_static(pos, text),
        }
    }

    /// Whether the compile structure dropped the parser node `id`.
    pub(super) fn is_dropped(&self, id: NodeId) -> bool {
        self.inputs
            .topology
            .node_for_syntax(id)
            .is_some_and(|node| self.dropped.get(node.index()).copied().unwrap_or(false))
    }

    /// The resolved child records of the node whose parser children are
    /// `children`, read from the compile structure.
    pub(super) fn build_child_records(
        &self,
        children: &[NodeId],
        source: &str,
    ) -> Vec<ChildRecord> {
        let topology = self.inputs.topology;
        let structure = self.inputs.structure;
        let Some(parent) = children
            .first()
            .and_then(|&child| topology.node_for_syntax(child))
            .and_then(|child| topology.parent(child))
        else {
            return Vec::new();
        };
        let resolved = structure.children(structure.compile_id(parent));
        let mut records = Vec::with_capacity(resolved.len());
        for child in resolved {
            let node = structure.template_id(child.node);
            let Some(syntax) = topology.syntax(node) else {
                continue;
            };
            let kind = match child.kind {
                VueChildKind::Element => ChildKind::Element,
                VueChildKind::Interpolation => ChildKind::Interpolation,
                VueChildKind::Comment => ChildKind::Comment,
                // Whitespace the structure keeps renders as one space,
                // recorded by `settle_children`.
                VueChildKind::Text
                | VueChildKind::WhitespaceNewline
                | VueChildKind::WhitespaceSpace => ChildKind::Text,
            };
            if let AstNodeKind::Element(el) = &self.ast.nodes[syntax.0].kind {
                let end = el
                    .tag_close
                    .as_ref()
                    .map(|tc| tc.end)
                    .unwrap_or(el.tag_open.end);
                let (condition, condition_prefix) = match el.v_condition.as_ref() {
                    Some(c) => {
                        let role = match c.kind {
                            ElementNodeConditionKind::If => ConditionChainRole::Start,
                            ElementNodeConditionKind::ElseIf | ElementNodeConditionKind::Else => {
                                ConditionChainRole::Continuation
                            }
                        };
                        // The ternary head of a `v-if`/`v-else-if`, from the
                        // expression plan `enter_element` resolved.
                        let prefix = match c.kind {
                            ElementNodeConditionKind::If | ElementNodeConditionKind::ElseIf => {
                                Some(self.condition_prefix_segments(syntax.0, c, source))
                            }
                            ElementNodeConditionKind::Else => None,
                        };
                        (Some(role), prefix)
                    }
                    None => (None, None),
                };
                records.push(ChildRecord {
                    start: el.tag_open.start,
                    end,
                    kind,
                    condition,
                    condition_prefix,
                });
            } else {
                let span = topology.span(node);
                records.push(ChildRecord {
                    start: span.start,
                    end: span.end,
                    kind,
                    condition: None,
                    condition_prefix: None,
                });
            }
        }
        records
    }

    /// Whether a `v-if`/`v-else-if` branch has a later branch in its chain.
    pub(super) fn has_next_condition_sibling(&self, id: NodeId) -> bool {
        let topology = self.inputs.topology;
        topology
            .node_for_syntax(id)
            .and_then(|node| topology.chain_of(node))
            .is_some_and(|(chain, index)| (index as usize) + 1 < topology.branches(chain).len())
    }

    /// Build the `(` … `) ? ` ternary head plan for a v-if/v-else-if element.
    ///
    /// Wraps the pre-resolved expression plan stored by `enter_element` (keyed
    /// by AST node index) in the synthetic ternary head, so only authored
    /// identifiers carry source-map tokens. `enter_element` always populates the
    /// map for If/ElseIf; the raw-expression branch is a defensive fallback.
    fn condition_prefix_segments(
        &self,
        node_idx: usize,
        c: &ElementNodeCondition,
        source: &str,
    ) -> MappedGeneratedText {
        match self.resolved_condition_prefixes.get(&node_idx) {
            Some(expr) => expr.wrapped("(", self.branch_question()),
            None => {
                let raw_expr = helpers::extract_directive_value(&c.prop, source);
                let value_start = c.prop.value_start.unwrap_or(0);
                let expr = if raw_expr.is_empty() {
                    MappedGeneratedText::synthetic("true")
                } else {
                    MappedGeneratedText::source(raw_expr, value_start)
                };
                expr.wrapped("(", self.branch_question())
            }
        }
    }
}
