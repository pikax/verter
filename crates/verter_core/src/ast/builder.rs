// ======================================================================
// Builder skeleton (stack of open *elements* only)
// - Leaves attach to current element (top of stack) or to root
// - Leaves can never be pushed to the open stack
// ======================================================================

use smallvec::SmallVec;

use crate::ast::types::{
    AstNode, AstNodeKind, ChildrenFlag, ChildrenFlags, ChildrenMode, CommentNode, ConditionalChain,
    ElementContent, ElementNode, ElementNodeCondition, ElementNodeConditionKind, InterpolationNode,
    PropFlag, PropFlags, TagType, TemplateAst, TextNode,
};
use crate::parser::types::RootNodeTemplate;
use crate::types::{NodeId, NodeProp, NodeTag};

/// Extract the `ElementNode` from an `AstNode` that is known to be an element.
/// Panics with a descriptive message if the invariant is violated.
#[inline]
fn element_mut(node: &mut AstNode) -> &mut ElementNode {
    match &mut node.kind {
        AstNodeKind::Element(el) => el,
        other => unreachable!(
            "builder invariant violated: expected Element node, found {:?}",
            std::mem::discriminant(other)
        ),
    }
}

#[inline]
fn element_ref(node: &AstNode) -> &ElementNode {
    match &node.kind {
        AstNodeKind::Element(el) => el,
        other => unreachable!(
            "builder invariant violated: expected Element node, found {:?}",
            std::mem::discriminant(other)
        ),
    }
}

/// Incremental AST builder driven by tokenizer events.
///
/// Maintains a stack of currently-open element [`NodeId`]s. Leaf nodes
/// (text, comments, interpolations) attach to the top-of-stack element,
/// or to the root if the stack is empty. Elements are pushed on open and
/// popped on close, at which point [`ChildrenFlag`] / [`ChildrenMode`]
/// metadata is computed from the element's children.
///
/// The builder does **not** perform close-tag name validation — that is
/// handled by [`super::super::syntax::Syntax`] which drives this builder.
pub struct TemplateAstBuilder {
    /// The AST being constructed. Public so the syntax layer can finalize
    /// root metadata (tag_close, content end) before calling [`finish`].
    pub ast: TemplateAst,
    /// Stack of open element `NodeId`s (elements only — leaves never pushed).
    open_stack: Vec<NodeId>,
}

impl TemplateAstBuilder {
    pub fn new(root: RootNodeTemplate) -> Self {
        Self {
            ast: TemplateAst::new(root),
            open_stack: Vec::with_capacity(8),
        }
    }

    /// Call on `OpenTagName`.
    pub fn open_element(&mut self, tag_open: NodeTag) {
        let id = self
            .ast
            .alloc_node(AstNodeKind::Element(Box::new(ElementNode {
                tag_open,
                tag_close: None,
                tag_type: TagType::Element, // default; overwritten by syntax layer
                is_self_closing: false,     // set by syntax layer on SelfClosingTag
                props: Vec::with_capacity(4),
                content: None,

                v_condition: None,
                v_for: None,
                v_slot: None,
                v_once: None,
                v_ref: None,
                prop_flag: PropFlag::empty(),
                children_flag: ChildrenFlag::empty(), // computed in close_element
                children_mode: ChildrenMode::Empty,   // computed in close_element
                is_fully_static: false,               // computed in close_element
            })));
        self.open_stack.push(id);
    }

    /// Update the current element's `tag_open.end` after the closing `>` is reached.
    ///
    /// The initial `open_element` call stores `tag_open.end = start` (the `<` position)
    /// because the `>` position is not known yet. This method patches it once
    /// `OpenTagEnd` or `SelfClosingTag` fires.
    pub fn set_tag_open_end(&mut self, end: u32) {
        let Some(&id) = self.open_stack.last() else {
            debug_assert!(false, "set_tag_open_end called with empty open_stack");
            return;
        };
        let el = element_mut(&mut self.ast.nodes[id.0]);
        el.tag_open.end = end;
    }

    /// Call on `OpenTagEnd` (or wherever you define content start).
    /// If you only want content when there are actual children, you can skip this and instead
    /// set content.start lazily when first child is attached. This version eagerly sets it.
    pub fn mark_element_content_start(&mut self, start: u32) {
        let Some(&id) = self.open_stack.last() else {
            return;
        };
        let el = element_mut(&mut self.ast.nodes[id.0]);

        el.content.get_or_insert_with(|| ElementContent {
            start,
            end: start,
            children: SmallVec::new(),
            v_if_chains: SmallVec::new(),
        });
    }

    /// Call on `SelfClosingTag` / `CloseTag` once you know end spans and optional close tag.
    /// Returns the `NodeId` of the closed element (for post-close validation).
    pub fn close_element(&mut self, tag_close: Option<NodeTag>, content_end: u32) -> NodeId {
        // Invariant: parser guarantees matched open/close — every CloseTag has a preceding OpenTag
        let id = self
            .open_stack
            .pop()
            .expect("close_element called with empty open_stack");

        let (children_flag, children_mode) = self.compute_children_meta(id);
        let is_fully_static = self.compute_is_fully_static(id, children_flag);

        // Scan for v-if chains among children
        let chains = if children_flag.has(ChildrenFlags::HasVIf) {
            let el = element_ref(&self.ast.nodes[id.0]);
            el.content
                .as_ref()
                .map(|c| scan_v_if_chains(&c.children, &self.ast.nodes))
                .unwrap_or_default()
        } else {
            SmallVec::new()
        };

        {
            let el = element_mut(&mut self.ast.nodes[id.0]);
            el.tag_close = tag_close;
            el.children_flag = children_flag;
            el.children_mode = children_mode;
            el.is_fully_static = is_fully_static;
            if let Some(content) = el.content.as_mut() {
                content.end = content_end;
                content.v_if_chains = chains;
            }
        }

        // Attach to parent element or root
        if let Some(&parent_id) = self.open_stack.last() {
            self.ast.attach_to_parent(parent_id, id);
        } else {
            self.ast.attach_to_root(id);
        }

        id
    }

    /// Compute children metadata by examining all children of the element at `id`.
    fn compute_children_meta(&self, id: NodeId) -> (ChildrenFlag, ChildrenMode) {
        let el = element_ref(&self.ast.nodes[id.0]);

        let Some(content) = &el.content else {
            let flag = ChildrenFlag::empty();
            return (flag, flag.mode());
        };

        let children = &content.children;
        if children.is_empty() {
            let flag = ChildrenFlag::empty();
            return (flag, flag.mode());
        }

        let mut flag = ChildrenFlag::empty();
        let mut significant_count: u32 = 0;

        for &child_id in children {
            let child = &self.ast.nodes[child_id.0];
            match &child.kind {
                AstNodeKind::Text(_) => {
                    flag = flag.add(ChildrenFlags::HasText);
                    significant_count += 1;
                }
                AstNodeKind::Interpolation(_) => {
                    flag = flag.add(ChildrenFlags::HasInterpolation);
                    significant_count += 1;
                }
                AstNodeKind::Element(child_el) => {
                    flag = flag.add(ChildrenFlags::HasElement);
                    significant_count += 1;
                    if child_el.v_condition.is_some() {
                        flag = flag.add(ChildrenFlags::HasVIf);
                    }
                    if child_el.v_for.is_some() {
                        flag = flag.add(ChildrenFlags::HasVFor);
                    }
                    if child_el.v_slot.is_some() {
                        flag = flag.add(ChildrenFlags::HasChildWithVSlot);
                        if child_el
                            .v_slot
                            .as_ref()
                            .and_then(|p| p.is_dynamic)
                            .unwrap_or(false)
                        {
                            flag = flag.add(ChildrenFlags::HasDynamicSlotChild);
                        }
                    }
                    if child_el.prop_flag.has(PropFlags::HasDynamicKey) {
                        flag = flag.add(ChildrenFlags::HasChildWithKey);
                    }
                }
                AstNodeKind::Comment(_) => {
                    flag = flag.add(ChildrenFlags::HasComment);
                    // comments are not significant for SingleChild
                }
            }
        }

        if significant_count == 1 {
            flag = flag.add(ChildrenFlags::SingleChild);
        }

        (flag, flag.mode())
    }

    /// Determine whether an element is fully static (eligible for static hoisting).
    ///
    /// An element is fully static when ALL of these hold:
    /// 1. Plain HTML element (`tag_type.is_element()`)
    /// 2. No structural directives (`v_condition`, `v_for`, `v_slot`, `v_once`, `v_ref`)
    /// 3. No dynamic props (no bits in `NEEDS_OXC_MASK`)
    /// 4. No interpolation children (`!HasInterpolation`)
    /// 5. No structural children (`!has_structural()`)
    /// 6. No child with v-slot (`!HasChildWithVSlot`)
    /// 7. All child elements have `is_fully_static == true`
    ///
    /// Text and comment nodes are inherently static.
    /// Children are always closed before parents, so child `is_fully_static` is available.
    fn compute_is_fully_static(&self, id: NodeId, children_flag: ChildrenFlag) -> bool {
        let el = element_ref(&self.ast.nodes[id.0]);

        // Rule 1: must be a plain HTML element
        if !el.tag_type.is_element() {
            return false;
        }

        // Rule 2: no structural directives
        if el.v_condition.is_some()
            || el.v_for.is_some()
            || el.v_slot.is_some()
            || el.v_once.is_some()
            || el.v_ref.is_some()
        {
            return false;
        }

        // Rule 3: no dynamic props
        if el.prop_flag.has_any(PropFlag::NEEDS_OXC_MASK) {
            return false;
        }

        // Rule 4: no interpolation children
        if children_flag.has(ChildrenFlags::HasInterpolation) {
            return false;
        }

        // Rule 5: no structural children
        if children_flag.has_structural() {
            return false;
        }

        // Rule 6: no child with v-slot
        if children_flag.has(ChildrenFlags::HasChildWithVSlot) {
            return false;
        }

        // Rule 7: all child elements must be fully static
        if let Some(content) = &el.content {
            for &child_id in &content.children {
                if let AstNodeKind::Element(child_el) = &self.ast.nodes[child_id.0].kind {
                    if !child_el.is_fully_static {
                        return false;
                    }
                }
                // Text and comment nodes are inherently static — no check needed
            }
        }

        true
    }

    /// Add a prop to the currently open element.
    pub fn push_prop_to_current(&mut self, prop: NodeProp) {
        let Some(&id) = self.open_stack.last() else {
            return;
        };
        element_mut(&mut self.ast.nodes[id.0]).props.push(prop);
    }

    // ---- built-in directive cache setters ----
    // Called by the syntax layer after bytes-only classification.
    // First occurrence wins; caller is responsible for duplicate detection.

    /// Set-if-empty helper for a cached directive field on the current element.
    /// Returns `true` if the field was already set (duplicate).
    fn set_cached_directive<T>(
        &mut self,
        value: T,
        field: impl FnOnce(&mut ElementNode) -> &mut Option<T>,
    ) -> bool {
        let Some(&id) = self.open_stack.last() else {
            return false;
        };
        let slot = field(element_mut(&mut self.ast.nodes[id.0]));
        if slot.is_some() {
            return true;
        }
        *slot = Some(value);
        false
    }

    /// Cache a v-if / v-else-if / v-else directive on the current element.
    /// Returns `true` if the field was already set (duplicate).
    pub fn set_v_condition(&mut self, condition: ElementNodeCondition) -> bool {
        self.set_cached_directive(condition, |el| &mut el.v_condition)
    }

    /// Cache a v-for directive on the current element.
    /// Returns `true` if the field was already set (duplicate).
    pub fn set_v_for(&mut self, prop: NodeProp) -> bool {
        self.set_cached_directive(prop, |el| &mut el.v_for)
    }

    /// Cache a v-slot directive on the current element.
    /// Returns `true` if the field was already set (duplicate).
    pub fn set_v_slot(&mut self, prop: NodeProp) -> bool {
        self.set_cached_directive(prop, |el| &mut el.v_slot)
    }

    /// Cache a v-once directive on the current element.
    /// Returns `true` if the field was already set (duplicate).
    pub fn set_v_once(&mut self, prop: NodeProp) -> bool {
        self.set_cached_directive(prop, |el| &mut el.v_once)
    }

    /// Cache a `ref` attribute on the current element.
    /// Returns `true` if the field was already set (duplicate).
    pub fn set_v_ref(&mut self, prop: NodeProp) -> bool {
        self.set_cached_directive(prop, |el| &mut el.v_ref)
    }

    /// Get the tag type of the currently open element.
    pub fn current_tag_type(&self) -> Option<TagType> {
        let id = self.open_stack.last()?;
        let node = &self.ast.nodes[id.0];
        if let AstNodeKind::Element(el) = &node.kind {
            Some(el.tag_type)
        } else {
            None
        }
    }

    // ---- tag metadata setters ----

    /// Set the tag type on the currently open element.
    pub fn set_tag_type(&mut self, tag_type: TagType) {
        let Some(&id) = self.open_stack.last() else {
            debug_assert!(false, "set_tag_type called with empty open_stack");
            return;
        };
        element_mut(&mut self.ast.nodes[id.0]).tag_type = tag_type;
    }

    /// Mark the currently open element as self-closing.
    pub fn set_self_closing(&mut self) {
        let Some(&id) = self.open_stack.last() else {
            debug_assert!(false, "set_self_closing called with empty open_stack");
            return;
        };
        element_mut(&mut self.ast.nodes[id.0]).is_self_closing = true;
    }

    // ---- prop flag setter ----

    /// Set a prop flag on the currently open element.
    pub fn add_prop_flag(&mut self, flag: PropFlags) {
        let Some(&id) = self.open_stack.last() else {
            debug_assert!(false, "add_prop_flag called with empty open_stack");
            return;
        };
        let el = element_mut(&mut self.ast.nodes[id.0]);
        el.prop_flag = el.prop_flag.add(flag);
    }

    // ------------------- leaf wiring -------------------

    fn attach_leaf(&mut self, leaf_id: NodeId) {
        if let Some(&parent_id) = self.open_stack.last() {
            self.ast.attach_to_parent(parent_id, leaf_id);
        } else {
            self.ast.attach_to_root(leaf_id);
        }
    }

    /// Call on `TokenizerEvent::Text { start, end }` and `TextEntity`.
    pub fn add_text(
        &mut self,
        start: u32,
        end: u32,
        is_entity: bool,
        is_whitespace_only: bool,
    ) -> NodeId {
        let id = self.ast.alloc_node(AstNodeKind::Text(TextNode {
            start,
            end,
            is_entity,
            is_whitespace_only,
        }));
        self.attach_leaf(id);
        id
    }

    /// Call on `TokenizerEvent::Comment { ... }`.
    pub fn add_comment(
        &mut self,
        start: u32,
        end: u32,
        content_start: u32,
        content_end: u32,
    ) -> NodeId {
        let id = self.ast.alloc_node(AstNodeKind::Comment(CommentNode {
            start,
            end,
            content_start,
            content_end,
        }));
        self.attach_leaf(id);
        id
    }

    /// Call on `TokenizerEvent::Interpolation { ... }`.
    pub fn add_interpolation(
        &mut self,
        start: u32,
        end: u32,
        inner_start: u32,
        inner_end: u32,
    ) -> NodeId {
        let id = self
            .ast
            .alloc_node(AstNodeKind::Interpolation(InterpolationNode {
                start,
                end,
                inner_start,
                inner_end,
            }));
        self.attach_leaf(id);
        id
    }

    // ---------------------------------------------------

    /// Returns true if there are still unclosed elements on the open stack.
    #[cfg(test)]
    pub fn has_open_elements(&self) -> bool {
        !self.open_stack.is_empty()
    }

    pub fn finish(mut self) -> TemplateAst {
        debug_assert!(
            self.open_stack.is_empty(),
            "TemplateAstBuilder::finish() called with {} unclosed element(s) on the open stack. \
             The caller must close all elements (or force-close on EOF) before finishing.",
            self.open_stack.len()
        );

        // Scan for v-if chains among root-level children
        if let Some(content) = self.ast.root.content.as_mut() {
            let has_v_if = content.children.iter().any(|&child_id| {
                matches!(
                    &self.ast.nodes[child_id.0].kind,
                    AstNodeKind::Element(el) if el.v_condition.is_some()
                )
            });
            if has_v_if {
                content.v_if_chains = scan_v_if_chains(&content.children, &self.ast.nodes);
            }
        }

        self.ast
    }
}

/// Scan a children list for v-if / v-else-if / v-else chains.
///
/// A chain starts at an element with `v-if`, continues through `v-else-if`/`v-else`
/// siblings, skipping whitespace-only text and comments. Non-whitespace text or
/// non-conditional elements break the chain.
fn scan_v_if_chains(children: &[NodeId], nodes: &[AstNode]) -> SmallVec<[ConditionalChain; 1]> {
    fn flush_chain(
        chains: &mut SmallVec<[ConditionalChain; 1]>,
        current_chain: &mut Option<SmallVec<[usize; 3]>>,
    ) {
        if let Some(members) = current_chain.take() {
            chains.push(ConditionalChain {
                member_indices: members,
            });
        }
    }

    let mut chains = SmallVec::new();
    let mut current_chain: Option<SmallVec<[usize; 3]>> = None;

    for (idx, &child_id) in children.iter().enumerate() {
        let node = &nodes[child_id.0];
        match &node.kind {
            AstNodeKind::Element(el) => {
                if let Some(ref cond) = el.v_condition {
                    match cond.kind {
                        ElementNodeConditionKind::If => {
                            // Flush any previous chain
                            flush_chain(&mut chains, &mut current_chain);
                            // Start new chain
                            let mut members = SmallVec::new();
                            members.push(idx);
                            current_chain = Some(members);
                        }
                        ElementNodeConditionKind::ElseIf | ElementNodeConditionKind::Else => {
                            // Continue existing chain
                            if let Some(ref mut members) = current_chain {
                                members.push(idx);
                            }
                            // If v-else, finalize the chain
                            if cond.kind == ElementNodeConditionKind::Else {
                                flush_chain(&mut chains, &mut current_chain);
                            }
                        }
                    }
                } else {
                    // Non-conditional element — break chain
                    flush_chain(&mut chains, &mut current_chain);
                }
            }
            AstNodeKind::Text(t) => {
                if !t.is_whitespace_only {
                    // Non-whitespace text breaks the chain
                    flush_chain(&mut chains, &mut current_chain);
                }
                // Whitespace-only text: skip (chain continues)
            }
            AstNodeKind::Comment(_) => {
                // Comments: skip (chain continues)
            }
            AstNodeKind::Interpolation(_) => {
                // Interpolation breaks the chain
                flush_chain(&mut chains, &mut current_chain);
            }
        }
    }

    // Flush any remaining chain (including solo v-if) for codegen.
    flush_chain(&mut chains, &mut current_chain);

    chains
}

#[cfg(test)]
#[path = "builder_tests.rs"]
mod builder_tests;
