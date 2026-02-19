// ======================================================================
// Builder skeleton (stack of open *elements* only)
// - Leaves attach to current element (top of stack) or to root
// - Leaves can never be pushed to the open stack
// ======================================================================

use smallvec::SmallVec;

use crate::new_impl::{
    ast::types::{
        AstNode, AstNodeKind, ChildrenFlag, ChildrenFlags, ChildrenMode, CommentNode,
        ElementContent, ElementNode, ElementNodeCondition, InterpolationNode, PropFlag, PropFlags,
        TagType, TemplateAst, TextNode,
    },
    syntax::types::RootNodeTemplate,
    types::{NodeId, NodeProp, NodeTag},
};

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
            open_stack: Vec::new(),
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
                props: Vec::new(),
                content: None,

                v_condition: None,
                v_for: None,
                v_slot: None,
                v_once: None,
                v_ref: None,
                prop_flag: PropFlag::empty(),
                children_flag: ChildrenFlag::empty(), // computed in close_element
                children_mode: ChildrenMode::Empty,   // computed in close_element
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
        });
    }

    /// Call on `SelfClosingTag` / `CloseTag` once you know end spans and optional close tag.
    /// Returns the `NodeId` of the closed element (for post-close validation).
    pub fn close_element(&mut self, tag_close: Option<NodeTag>, content_end: u32) -> NodeId {
        let id = self
            .open_stack
            .pop()
            .expect("close_element called with empty open_stack");

        let (children_flag, children_mode) = self.compute_children_meta(id);

        {
            let el = element_mut(&mut self.ast.nodes[id.0]);
            el.tag_close = tag_close;
            el.children_flag = children_flag;
            el.children_mode = children_mode;
            if let Some(content) = el.content.as_mut() {
                content.end = content_end;
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

    // ---- tag metadata setters ----

    /// Set the tag type on the currently open element.
    pub fn set_tag_type(&mut self, tag_type: TagType) {
        let Some(&id) = self.open_stack.last() else {
            return;
        };
        element_mut(&mut self.ast.nodes[id.0]).tag_type = tag_type;
    }

    /// Mark the currently open element as self-closing.
    pub fn set_self_closing(&mut self) {
        let Some(&id) = self.open_stack.last() else {
            return;
        };
        element_mut(&mut self.ast.nodes[id.0]).is_self_closing = true;
    }

    // ---- prop flag setter ----

    /// Set a prop flag on the currently open element.
    pub fn add_prop_flag(&mut self, flag: PropFlags) {
        let Some(&id) = self.open_stack.last() else {
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
    pub fn add_text(&mut self, start: u32, end: u32, is_entity: bool) -> NodeId {
        let id = self.ast.alloc_node(AstNodeKind::Text(TextNode {
            start,
            end,
            is_entity,
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
    pub fn has_open_elements(&self) -> bool {
        !self.open_stack.is_empty()
    }

    pub fn finish(self) -> TemplateAst {
        debug_assert!(
            self.open_stack.is_empty(),
            "TemplateAstBuilder::finish() called with {} unclosed element(s) on the open stack. \
             The caller must close all elements (or force-close on EOF) before finishing.",
            self.open_stack.len()
        );
        self.ast
    }
}

#[cfg(test)]
mod builder_tests {
    use super::*;
    use crate::new_impl::ast::types::{
        ChildrenMode, ElementNodeCondition, ElementNodeConditionKind,
    };
    use crate::new_impl::test_helpers::{make_root, make_tag};
    use smallvec::SmallVec;

    /// Helper: create a simple NodeProp (non-directive attribute).
    fn make_attr(start: u32, name_end: u32) -> NodeProp {
        NodeProp {
            start,
            name_end,
            is_directive: false,
            arg_start: None,
            arg_end: None,
            is_dynamic: None,
            value_start: None,
            value_end: None,
            modifiers: SmallVec::new(),
        }
    }

    // ========================================================================
    // 1. Basic open/close element
    // ========================================================================

    /// @ai-generated - Tests basic open + close element attached to root.
    #[test]
    fn open_close_single_element() {
        let mut b = TemplateAstBuilder::new(make_root());

        b.open_element(make_tag(0, 5, 4));
        b.mark_element_content_start(5);
        b.close_element(Some(make_tag(5, 11, 10)), 5);

        let ast = b.finish();

        let root_children = ast.root.content.as_ref().unwrap();
        assert_eq!(root_children.children.len(), 1);

        let node = &ast.nodes[root_children.children[0].0];
        assert!(node.parent.is_none()); // root child
        assert_eq!(node.index_in_parent, 0);

        let AstNodeKind::Element(el) = &node.kind else {
            panic!("expected Element");
        };
        assert!(el.tag_close.is_some());
        assert!(el.content.is_some());
        let content = el.content.as_ref().unwrap();
        assert_eq!(content.start, 5);
        assert_eq!(content.end, 5);
        assert!(content.children.is_empty());
    }

    // ========================================================================
    // 2. Self-closing element (no close tag)
    // ========================================================================

    /// @ai-generated - Tests self-closing element (tag_close = None).
    #[test]
    fn self_closing_element() {
        let mut b = TemplateAstBuilder::new(make_root());

        b.open_element(make_tag(0, 10, 4));
        b.close_element(None, 10);

        let ast = b.finish();
        let root_children = ast.root.content.as_ref().unwrap();
        assert_eq!(root_children.children.len(), 1);

        let AstNodeKind::Element(el) = &ast.nodes[root_children.children[0].0].kind else {
            panic!("expected Element");
        };
        assert!(el.tag_close.is_none());
        // No content was marked
        assert!(el.content.is_none());
    }

    // ========================================================================
    // 3. Nested elements
    // ========================================================================

    /// @ai-generated - Tests nested element parent/child relationships.
    #[test]
    fn nested_elements() {
        let mut b = TemplateAstBuilder::new(make_root());

        // <div>
        b.open_element(make_tag(0, 5, 4));
        b.mark_element_content_start(5);

        //   <span>
        b.open_element(make_tag(5, 11, 10));
        b.mark_element_content_start(11);
        //   </span>
        b.close_element(Some(make_tag(11, 18, 17)), 11);

        // </div>
        b.close_element(Some(make_tag(18, 24, 23)), 18);

        let ast = b.finish();

        // Root has 1 child (div)
        let root_children = ast.root.content.as_ref().unwrap();
        assert_eq!(root_children.children.len(), 1);

        let div_id = root_children.children[0];
        let div_node = &ast.nodes[div_id.0];
        assert!(div_node.parent.is_none());

        let AstNodeKind::Element(div_el) = &div_node.kind else {
            panic!("expected Element for div");
        };
        let div_content = div_el.content.as_ref().unwrap();
        assert_eq!(div_content.children.len(), 1);

        // span is child of div
        let span_id = div_content.children[0];
        let span_node = &ast.nodes[span_id.0];
        assert_eq!(span_node.parent, Some(div_id));
        assert_eq!(span_node.index_in_parent, 0);
    }

    // ========================================================================
    // 4. Deeply nested elements (3 levels)
    // ========================================================================

    /// @ai-generated - Tests 3-level nesting.
    #[test]
    fn deeply_nested_elements() {
        let mut b = TemplateAstBuilder::new(make_root());

        // <a>
        b.open_element(make_tag(0, 3, 2));
        b.mark_element_content_start(3);
        //   <b>
        b.open_element(make_tag(3, 6, 5));
        b.mark_element_content_start(6);
        //     <c />
        b.open_element(make_tag(6, 10, 8));
        b.close_element(None, 10);
        //   </b>
        b.close_element(Some(make_tag(10, 14, 13)), 10);
        // </a>
        b.close_element(Some(make_tag(14, 18, 17)), 14);

        let ast = b.finish();

        let root_children = ast.root.content.as_ref().unwrap();
        assert_eq!(root_children.children.len(), 1);

        let a_id = root_children.children[0];
        let AstNodeKind::Element(a) = &ast.nodes[a_id.0].kind else {
            panic!("expected Element");
        };
        let b_id = a.content.as_ref().unwrap().children[0];
        let AstNodeKind::Element(b_el) = &ast.nodes[b_id.0].kind else {
            panic!("expected Element");
        };
        let c_id = b_el.content.as_ref().unwrap().children[0];
        let c_node = &ast.nodes[c_id.0];
        assert_eq!(c_node.parent, Some(b_id));
    }

    // ========================================================================
    // 5. Leaf nodes attached to root
    // ========================================================================

    /// @ai-generated - Tests leaf nodes attach to root when no element is open.
    #[test]
    fn leaves_attach_to_root() {
        let mut b = TemplateAstBuilder::new(make_root());

        let text_id = b.add_text(0, 5, false);
        let comment_id = b.add_comment(5, 20, 9, 16);
        let interp_id = b.add_interpolation(20, 30, 22, 28);

        let ast = b.finish();

        let root_children = ast.root.content.as_ref().unwrap();
        assert_eq!(root_children.children.len(), 3);
        assert_eq!(root_children.children[0], text_id);
        assert_eq!(root_children.children[1], comment_id);
        assert_eq!(root_children.children[2], interp_id);

        // All root children have parent = None
        assert!(ast.nodes[text_id.0].parent.is_none());
        assert!(ast.nodes[comment_id.0].parent.is_none());
        assert!(ast.nodes[interp_id.0].parent.is_none());

        // Verify index_in_parent
        assert_eq!(ast.nodes[text_id.0].index_in_parent, 0);
        assert_eq!(ast.nodes[comment_id.0].index_in_parent, 1);
        assert_eq!(ast.nodes[interp_id.0].index_in_parent, 2);
    }

    // ========================================================================
    // 6. Leaf nodes attached to open element
    // ========================================================================

    /// @ai-generated - Tests leaf nodes attach to the currently open element.
    #[test]
    fn leaves_attach_to_open_element() {
        let mut b = TemplateAstBuilder::new(make_root());

        b.open_element(make_tag(0, 5, 4));
        b.mark_element_content_start(5);

        let text_id = b.add_text(5, 10, false);
        let interp_id = b.add_interpolation(10, 20, 12, 18);

        b.close_element(Some(make_tag(20, 26, 25)), 20);

        let ast = b.finish();

        let div_id = ast.root.content.as_ref().unwrap().children[0];
        let AstNodeKind::Element(el) = &ast.nodes[div_id.0].kind else {
            panic!("expected Element");
        };
        let children = &el.content.as_ref().unwrap().children;
        assert_eq!(children.len(), 2);

        // Both leaves have parent = div
        assert_eq!(ast.nodes[text_id.0].parent, Some(div_id));
        assert_eq!(ast.nodes[interp_id.0].parent, Some(div_id));
        assert_eq!(ast.nodes[text_id.0].index_in_parent, 0);
        assert_eq!(ast.nodes[interp_id.0].index_in_parent, 1);
    }

    // ========================================================================
    // 7. push_prop_to_current
    // ========================================================================

    /// @ai-generated - Tests push_prop_to_current adds props to the open element.
    #[test]
    fn push_prop_to_current() {
        let mut b = TemplateAstBuilder::new(make_root());

        b.open_element(make_tag(0, 5, 4));
        b.push_prop_to_current(make_attr(5, 10));
        b.push_prop_to_current(make_attr(11, 16));
        b.mark_element_content_start(17);
        b.close_element(Some(make_tag(17, 23, 22)), 17);

        let ast = b.finish();
        let div_id = ast.root.content.as_ref().unwrap().children[0];
        let AstNodeKind::Element(el) = &ast.nodes[div_id.0].kind else {
            panic!("expected Element");
        };
        assert_eq!(el.props.len(), 2);
        assert_eq!(el.props[0].start, 5);
        assert_eq!(el.props[1].start, 11);
    }

    /// @ai-generated - Tests push_prop_to_current is a no-op with empty stack.
    #[test]
    fn push_prop_no_open_element_is_noop() {
        let mut b = TemplateAstBuilder::new(make_root());
        b.push_prop_to_current(make_attr(0, 5));
        let ast = b.finish();
        // No nodes were allocated — prop was silently dropped
        assert!(ast.nodes.is_empty());
    }

    // ========================================================================
    // 8. has_open_elements
    // ========================================================================

    /// @ai-generated - Tests has_open_elements tracking.
    #[test]
    fn has_open_elements_tracking() {
        let mut b = TemplateAstBuilder::new(make_root());
        assert!(!b.has_open_elements());

        b.open_element(make_tag(0, 5, 4));
        assert!(b.has_open_elements());

        b.open_element(make_tag(5, 10, 9));
        assert!(b.has_open_elements());

        b.close_element(None, 10);
        assert!(b.has_open_elements()); // outer still open

        b.close_element(Some(make_tag(10, 16, 15)), 10);
        assert!(!b.has_open_elements());
    }

    // ========================================================================
    // 9. finish with empty builder
    // ========================================================================

    /// @ai-generated - Tests finish on an empty builder produces empty AST.
    #[test]
    fn finish_empty_builder() {
        let b = TemplateAstBuilder::new(make_root());
        let ast = b.finish();
        assert!(ast.nodes.is_empty());
        assert!(ast.root.content.as_ref().unwrap().children.is_empty());
    }

    // ========================================================================
    // 10. Children flags: single text child
    // ========================================================================

    /// @ai-generated - Tests children flags for a single text child.
    #[test]
    fn children_flag_single_text() {
        let mut b = TemplateAstBuilder::new(make_root());

        b.open_element(make_tag(0, 5, 4));
        b.mark_element_content_start(5);
        b.add_text(5, 10, false);
        b.close_element(Some(make_tag(10, 16, 15)), 10);

        let ast = b.finish();
        let div_id = ast.root.content.as_ref().unwrap().children[0];
        let AstNodeKind::Element(el) = &ast.nodes[div_id.0].kind else {
            panic!("expected Element");
        };

        assert!(el.children_flag.has(ChildrenFlags::HasText));
        assert!(el.children_flag.has(ChildrenFlags::SingleChild));
        assert!(el.children_flag.is_text_only());
        assert!(!el.children_flag.has_dynamic());
    }

    // ========================================================================
    // 11. Children flags: single element child
    // ========================================================================

    /// @ai-generated - Tests children flags for a single element child.
    #[test]
    fn children_flag_single_element() {
        let mut b = TemplateAstBuilder::new(make_root());

        // <div>
        b.open_element(make_tag(0, 5, 4));
        b.mark_element_content_start(5);
        //   <span />
        b.open_element(make_tag(5, 12, 10));
        b.close_element(None, 12);
        // </div>
        b.close_element(Some(make_tag(12, 18, 17)), 12);

        let ast = b.finish();
        let div_id = ast.root.content.as_ref().unwrap().children[0];
        let AstNodeKind::Element(el) = &ast.nodes[div_id.0].kind else {
            panic!("expected Element");
        };

        assert!(el.children_flag.has(ChildrenFlags::HasElement));
        assert!(el.children_flag.has(ChildrenFlags::SingleChild));
        assert!(el.children_flag.needs_array());
        assert!(!el.children_flag.is_text_only());
    }

    // ========================================================================
    // 12. Children flags: multiple text children (no SingleChild)
    // ========================================================================

    /// @ai-generated - Tests that multiple text children don't set SingleChild.
    #[test]
    fn children_flag_multiple_text_no_single_child() {
        let mut b = TemplateAstBuilder::new(make_root());

        b.open_element(make_tag(0, 5, 4));
        b.mark_element_content_start(5);
        b.add_text(5, 10, false);
        b.add_text(10, 15, false);
        b.close_element(Some(make_tag(15, 21, 20)), 15);

        let ast = b.finish();
        let div_id = ast.root.content.as_ref().unwrap().children[0];
        let AstNodeKind::Element(el) = &ast.nodes[div_id.0].kind else {
            panic!("expected Element");
        };

        assert!(el.children_flag.has(ChildrenFlags::HasText));
        assert!(
            !el.children_flag.has(ChildrenFlags::SingleChild),
            "two text children should not set SingleChild"
        );
    }

    // ========================================================================
    // 13. Children flags: comment only (not significant)
    // ========================================================================

    /// @ai-generated - Tests that comment-only children don't set SingleChild.
    #[test]
    fn children_flag_comment_only() {
        let mut b = TemplateAstBuilder::new(make_root());

        b.open_element(make_tag(0, 5, 4));
        b.mark_element_content_start(5);
        b.add_comment(5, 20, 9, 16);
        b.close_element(Some(make_tag(20, 26, 25)), 20);

        let ast = b.finish();
        let div_id = ast.root.content.as_ref().unwrap().children[0];
        let AstNodeKind::Element(el) = &ast.nodes[div_id.0].kind else {
            panic!("expected Element");
        };

        assert!(el.children_flag.has(ChildrenFlags::HasComment));
        assert!(
            !el.children_flag.has(ChildrenFlags::SingleChild),
            "comments are not significant for SingleChild"
        );
        assert!(!el.children_flag.is_text_only());
    }

    // ========================================================================
    // 14. Children flags: text + comment → SingleChild (comment not significant)
    // ========================================================================

    /// @ai-generated - Tests that text + comment = SingleChild (only text is significant).
    #[test]
    fn children_flag_text_plus_comment_is_single() {
        let mut b = TemplateAstBuilder::new(make_root());

        b.open_element(make_tag(0, 5, 4));
        b.mark_element_content_start(5);
        b.add_text(5, 10, false);
        b.add_comment(10, 25, 14, 21);
        b.close_element(Some(make_tag(25, 31, 30)), 25);

        let ast = b.finish();
        let div_id = ast.root.content.as_ref().unwrap().children[0];
        let AstNodeKind::Element(el) = &ast.nodes[div_id.0].kind else {
            panic!("expected Element");
        };

        assert!(el.children_flag.has(ChildrenFlags::HasText));
        assert!(el.children_flag.has(ChildrenFlags::HasComment));
        assert!(
            el.children_flag.has(ChildrenFlags::SingleChild),
            "text + comment should still be SingleChild (comment not significant)"
        );
    }

    // ========================================================================
    // 15. Children flags: element with v_condition → HasVIf
    // ========================================================================

    /// @ai-generated - Tests that child element with v_condition sets HasVIf.
    #[test]
    fn children_flag_v_if() {
        let mut b = TemplateAstBuilder::new(make_root());

        // <div>  — node id 0
        b.open_element(make_tag(0, 5, 4));
        b.mark_element_content_start(5);

        //   <span v-if>  — node id 1
        b.open_element(make_tag(5, 11, 10));
        b.mark_element_content_start(11);
        b.close_element(Some(make_tag(11, 18, 17)), 11);

        // span is now closed and attached to div. Set v_condition on the span
        // node before closing div so compute_children_flags sees it.
        let span_id = NodeId(1);
        if let AstNodeKind::Element(span_el) = &mut b.ast.nodes[span_id.0].kind {
            span_el.v_condition = Some(ElementNodeCondition {
                kind: ElementNodeConditionKind::If,
                prop: make_attr(0, 4),
            });
        }

        // </div> — now compute_children_flags should see span's v_condition
        b.close_element(Some(make_tag(18, 24, 23)), 18);

        let ast = b.finish();
        let div_id = ast.root.content.as_ref().unwrap().children[0];
        let AstNodeKind::Element(el) = &ast.nodes[div_id.0].kind else {
            panic!("expected Element");
        };

        assert!(el.children_flag.has(ChildrenFlags::HasElement));
        assert!(el.children_flag.has(ChildrenFlags::HasVIf));
        assert!(el.children_flag.has(ChildrenFlags::SingleChild));
    }

    // ========================================================================
    // 16. Children flags: element with v_for → HasVFor
    // ========================================================================

    /// @ai-generated - Tests that child element with v_for sets HasVFor.
    #[test]
    fn children_flag_v_for() {
        let mut b = TemplateAstBuilder::new(make_root());

        b.open_element(make_tag(0, 5, 4));
        b.mark_element_content_start(5);

        b.open_element(make_tag(5, 11, 10));
        b.mark_element_content_start(11);
        b.close_element(Some(make_tag(11, 18, 17)), 11);

        // Set v_for on the child element before closing parent
        let child_id = NodeId(1);
        if let AstNodeKind::Element(child_el) = &mut b.ast.nodes[child_id.0].kind {
            child_el.v_for = Some(make_attr(0, 5));
        }

        b.close_element(Some(make_tag(18, 24, 23)), 18);

        let ast = b.finish();
        let div_id = ast.root.content.as_ref().unwrap().children[0];
        let AstNodeKind::Element(el) = &ast.nodes[div_id.0].kind else {
            panic!("expected Element");
        };

        assert!(el.children_flag.has(ChildrenFlags::HasElement));
        assert!(el.children_flag.has(ChildrenFlags::HasVFor));
    }

    // ========================================================================
    // 17. Children flags: mixed children (no SingleChild)
    // ========================================================================

    /// @ai-generated - Tests mixed text + element children flags.
    #[test]
    fn children_flag_mixed_text_and_element() {
        let mut b = TemplateAstBuilder::new(make_root());

        b.open_element(make_tag(0, 5, 4));
        b.mark_element_content_start(5);

        b.add_text(5, 10, false);
        b.open_element(make_tag(10, 16, 15));
        b.close_element(None, 16);

        b.close_element(Some(make_tag(16, 22, 21)), 16);

        let ast = b.finish();
        let div_id = ast.root.content.as_ref().unwrap().children[0];
        let AstNodeKind::Element(el) = &ast.nodes[div_id.0].kind else {
            panic!("expected Element");
        };

        assert!(el.children_flag.has(ChildrenFlags::HasText));
        assert!(el.children_flag.has(ChildrenFlags::HasElement));
        assert!(
            !el.children_flag.has(ChildrenFlags::SingleChild),
            "text + element = 2 significant children"
        );
        assert!(!el.children_flag.is_text_only());
        assert!(el.children_flag.needs_array());
    }

    // ========================================================================
    // 18. Children flags: text + interpolation (text_only + dynamic)
    // ========================================================================

    /// @ai-generated - Tests text + interpolation children flags.
    #[test]
    fn children_flag_text_and_interpolation() {
        let mut b = TemplateAstBuilder::new(make_root());

        b.open_element(make_tag(0, 5, 4));
        b.mark_element_content_start(5);

        b.add_text(5, 10, false);
        b.add_interpolation(10, 20, 12, 18);

        b.close_element(Some(make_tag(20, 26, 25)), 20);

        let ast = b.finish();
        let div_id = ast.root.content.as_ref().unwrap().children[0];
        let AstNodeKind::Element(el) = &ast.nodes[div_id.0].kind else {
            panic!("expected Element");
        };

        assert!(el.children_flag.has(ChildrenFlags::HasText));
        assert!(el.children_flag.has(ChildrenFlags::HasInterpolation));
        assert!(el.children_flag.is_text_only());
        assert!(el.children_flag.has_dynamic());
        assert!(!el.children_flag.needs_array());
    }

    // ========================================================================
    // 19. Children flags: no content → empty flags
    // ========================================================================

    /// @ai-generated - Tests that self-closing element has empty children flags.
    #[test]
    fn children_flag_no_content() {
        let mut b = TemplateAstBuilder::new(make_root());

        b.open_element(make_tag(0, 10, 4));
        // No mark_element_content_start → no content
        b.close_element(None, 10);

        let ast = b.finish();
        let div_id = ast.root.content.as_ref().unwrap().children[0];
        let AstNodeKind::Element(el) = &ast.nodes[div_id.0].kind else {
            panic!("expected Element");
        };

        assert_eq!(el.children_flag, ChildrenFlag::empty());
        assert!(!el.children_flag.has_children());
    }

    // ========================================================================
    // 20. Children flags: content marked but no children → empty flags
    // ========================================================================

    /// @ai-generated - Tests empty content (no children) produces empty flags.
    #[test]
    fn children_flag_empty_content() {
        let mut b = TemplateAstBuilder::new(make_root());

        b.open_element(make_tag(0, 5, 4));
        b.mark_element_content_start(5);
        // No children added
        b.close_element(Some(make_tag(5, 11, 10)), 5);

        let ast = b.finish();
        let div_id = ast.root.content.as_ref().unwrap().children[0];
        let AstNodeKind::Element(el) = &ast.nodes[div_id.0].kind else {
            panic!("expected Element");
        };

        assert_eq!(el.children_flag, ChildrenFlag::empty());
        assert_eq!(el.children_mode, ChildrenMode::Empty);
    }

    /// @ai-generated - Tests that comments-only children get the dedicated mode.
    #[test]
    fn children_mode_comments_only() {
        let mut b = TemplateAstBuilder::new(make_root());

        b.open_element(make_tag(0, 5, 4));
        b.mark_element_content_start(5);
        b.add_comment(5, 20, 9, 16);
        b.close_element(Some(make_tag(20, 26, 25)), 20);

        let ast = b.finish();
        let div_id = ast.root.content.as_ref().unwrap().children[0];
        let AstNodeKind::Element(el) = &ast.nodes[div_id.0].kind else {
            panic!("expected Element");
        };

        assert_eq!(el.children_mode, ChildrenMode::CommentsOnly);
    }

    /// @ai-generated - Tests that mixed children mode is precomputed in builder.
    #[test]
    fn children_mode_mixed() {
        let mut b = TemplateAstBuilder::new(make_root());

        b.open_element(make_tag(0, 5, 4));
        b.mark_element_content_start(5);
        b.add_text(5, 10, false);
        b.open_element(make_tag(10, 16, 15));
        b.close_element(None, 16);
        b.close_element(Some(make_tag(16, 22, 21)), 16);

        let ast = b.finish();
        let div_id = ast.root.content.as_ref().unwrap().children[0];
        let AstNodeKind::Element(el) = &ast.nodes[div_id.0].kind else {
            panic!("expected Element");
        };

        assert_eq!(el.children_mode, ChildrenMode::Mixed);
    }

    // ========================================================================
    // 21. mark_element_content_start with empty stack is no-op
    // ========================================================================

    /// @ai-generated - Tests mark_element_content_start with empty stack.
    #[test]
    fn mark_content_start_empty_stack_noop() {
        let mut b = TemplateAstBuilder::new(make_root());
        // Should not panic
        b.mark_element_content_start(42);
        let ast = b.finish();
        assert!(ast.nodes.is_empty());
    }

    // ========================================================================
    // 22. Multiple root children
    // ========================================================================

    /// @ai-generated - Tests multiple elements attached to root.
    #[test]
    fn multiple_root_children() {
        let mut b = TemplateAstBuilder::new(make_root());

        // First element
        b.open_element(make_tag(0, 5, 4));
        b.mark_element_content_start(5);
        b.close_element(Some(make_tag(5, 11, 10)), 5);

        // Text leaf at root level
        b.add_text(11, 15, false);

        // Second element
        b.open_element(make_tag(15, 21, 20));
        b.mark_element_content_start(21);
        b.close_element(Some(make_tag(21, 28, 27)), 21);

        let ast = b.finish();
        let root_children = ast.root.content.as_ref().unwrap();
        assert_eq!(root_children.children.len(), 3);

        // Verify index_in_parent
        assert_eq!(ast.nodes[root_children.children[0].0].index_in_parent, 0);
        assert_eq!(ast.nodes[root_children.children[1].0].index_in_parent, 1);
        assert_eq!(ast.nodes[root_children.children[2].0].index_in_parent, 2);

        // First and third are elements, second is text
        assert!(matches!(
            ast.nodes[root_children.children[0].0].kind,
            AstNodeKind::Element(_)
        ));
        assert!(matches!(
            ast.nodes[root_children.children[1].0].kind,
            AstNodeKind::Text(_)
        ));
        assert!(matches!(
            ast.nodes[root_children.children[2].0].kind,
            AstNodeKind::Element(_)
        ));
    }

    // ========================================================================
    // 23. Text entity flag
    // ========================================================================

    /// @ai-generated - Tests that text entity flag is preserved.
    #[test]
    fn text_entity_flag() {
        let mut b = TemplateAstBuilder::new(make_root());

        b.add_text(0, 5, false);
        b.add_text(5, 10, true);

        let ast = b.finish();
        let AstNodeKind::Text(t0) = &ast.nodes[0].kind else {
            panic!("expected Text");
        };
        let AstNodeKind::Text(t1) = &ast.nodes[1].kind else {
            panic!("expected Text");
        };

        assert!(!t0.is_entity);
        assert!(t1.is_entity);
    }

    // ========================================================================
    // 24. Interpolation spans
    // ========================================================================

    /// @ai-generated - Tests interpolation inner spans are preserved.
    #[test]
    fn interpolation_spans() {
        let mut b = TemplateAstBuilder::new(make_root());

        let id = b.add_interpolation(0, 12, 2, 10);

        let ast = b.finish();
        let AstNodeKind::Interpolation(i) = &ast.nodes[id.0].kind else {
            panic!("expected Interpolation");
        };
        assert_eq!(i.start, 0);
        assert_eq!(i.end, 12);
        assert_eq!(i.inner_start, 2);
        assert_eq!(i.inner_end, 10);
    }

    // ========================================================================
    // 25. Comment spans
    // ========================================================================

    /// @ai-generated - Tests comment content spans are preserved.
    #[test]
    fn comment_spans() {
        let mut b = TemplateAstBuilder::new(make_root());

        let id = b.add_comment(0, 20, 4, 16);

        let ast = b.finish();
        let AstNodeKind::Comment(c) = &ast.nodes[id.0].kind else {
            panic!("expected Comment");
        };
        assert_eq!(c.start, 0);
        assert_eq!(c.end, 20);
        assert_eq!(c.content_start, 4);
        assert_eq!(c.content_end, 16);
    }

    // ========================================================================
    // 26. set_v_condition — first wins, duplicate returns true
    // ========================================================================

    /// @ai-generated - Tests set_v_condition caches first and reports duplicate.
    #[test]
    fn set_v_condition_first_wins() {
        let mut b = TemplateAstBuilder::new(make_root());
        b.open_element(make_tag(0, 5, 4));

        let first = ElementNodeCondition {
            kind: ElementNodeConditionKind::If,
            prop: make_attr(5, 9),
        };
        let second = ElementNodeCondition {
            kind: ElementNodeConditionKind::ElseIf,
            prop: make_attr(10, 19),
        };

        assert!(
            !b.set_v_condition(first.clone()),
            "first set should return false"
        );
        assert!(b.set_v_condition(second), "duplicate should return true");

        b.mark_element_content_start(20);
        b.close_element(Some(make_tag(20, 26, 25)), 20);

        let ast = b.finish();
        let el_id = ast.root.content.as_ref().unwrap().children[0];
        let AstNodeKind::Element(el) = &ast.nodes[el_id.0].kind else {
            panic!("expected Element");
        };
        // First occurrence wins
        assert_eq!(
            el.v_condition.as_ref().unwrap().kind,
            ElementNodeConditionKind::If
        );
        assert_eq!(el.v_condition.as_ref().unwrap().prop.start, 5);
    }

    // ========================================================================
    // 27. set_v_for — first wins, duplicate returns true
    // ========================================================================

    /// @ai-generated - Tests set_v_for caches first and reports duplicate.
    #[test]
    fn set_v_for_first_wins() {
        let mut b = TemplateAstBuilder::new(make_root());
        b.open_element(make_tag(0, 5, 4));

        assert!(
            !b.set_v_for(make_attr(5, 10)),
            "first set should return false"
        );
        assert!(
            b.set_v_for(make_attr(11, 16)),
            "duplicate should return true"
        );

        b.mark_element_content_start(17);
        b.close_element(Some(make_tag(17, 23, 22)), 17);

        let ast = b.finish();
        let el_id = ast.root.content.as_ref().unwrap().children[0];
        let AstNodeKind::Element(el) = &ast.nodes[el_id.0].kind else {
            panic!("expected Element");
        };
        assert_eq!(el.v_for.as_ref().unwrap().start, 5);
    }

    // ========================================================================
    // 28. set_v_slot — first wins, duplicate returns true
    // ========================================================================

    /// @ai-generated - Tests set_v_slot caches first and reports duplicate.
    #[test]
    fn set_v_slot_first_wins() {
        let mut b = TemplateAstBuilder::new(make_root());
        b.open_element(make_tag(0, 5, 4));

        assert!(
            !b.set_v_slot(make_attr(5, 11)),
            "first set should return false"
        );
        assert!(
            b.set_v_slot(make_attr(12, 18)),
            "duplicate should return true"
        );

        b.mark_element_content_start(19);
        b.close_element(Some(make_tag(19, 25, 24)), 19);

        let ast = b.finish();
        let el_id = ast.root.content.as_ref().unwrap().children[0];
        let AstNodeKind::Element(el) = &ast.nodes[el_id.0].kind else {
            panic!("expected Element");
        };
        assert_eq!(el.v_slot.as_ref().unwrap().start, 5);
    }

    // ========================================================================
    // 29. set_v_once — first wins, duplicate returns true
    // ========================================================================

    /// @ai-generated - Tests set_v_once caches first and reports duplicate.
    #[test]
    fn set_v_once_first_wins() {
        let mut b = TemplateAstBuilder::new(make_root());
        b.open_element(make_tag(0, 5, 4));

        assert!(
            !b.set_v_once(make_attr(5, 11)),
            "first set should return false"
        );
        assert!(
            b.set_v_once(make_attr(12, 18)),
            "duplicate should return true"
        );

        b.mark_element_content_start(19);
        b.close_element(Some(make_tag(19, 25, 24)), 19);

        let ast = b.finish();
        let el_id = ast.root.content.as_ref().unwrap().children[0];
        let AstNodeKind::Element(el) = &ast.nodes[el_id.0].kind else {
            panic!("expected Element");
        };
        assert!(el.v_once.is_some());
        assert_eq!(el.v_once.as_ref().unwrap().start, 5);
    }

    // ========================================================================
    // 29b. set_v_ref — first wins, duplicate returns true
    // ========================================================================

    /// @ai-generated - Tests set_v_ref caches first and reports duplicate.
    #[test]
    fn set_v_ref_first_wins() {
        let mut b = TemplateAstBuilder::new(make_root());
        b.open_element(make_tag(0, 5, 4));

        assert!(
            !b.set_v_ref(make_attr(5, 8)),
            "first set should return false"
        );
        assert!(
            b.set_v_ref(make_attr(12, 15)),
            "duplicate should return true"
        );

        b.mark_element_content_start(19);
        b.close_element(Some(make_tag(19, 25, 24)), 19);

        let ast = b.finish();
        let el_id = ast.root.content.as_ref().unwrap().children[0];
        let AstNodeKind::Element(el) = &ast.nodes[el_id.0].kind else {
            panic!("expected Element");
        };
        assert!(el.v_ref.is_some());
        assert_eq!(el.v_ref.as_ref().unwrap().start, 5);
        // ref should NOT be in the props vec (it was taken out into v_ref)
    }

    // ========================================================================
    // 30. set_v_* with empty stack — no-op, returns false
    // ========================================================================

    /// @ai-generated - Tests cache setters are no-op with empty stack.
    #[test]
    fn set_v_cache_empty_stack_noop() {
        let mut b = TemplateAstBuilder::new(make_root());
        assert!(!b.set_v_condition(ElementNodeCondition {
            kind: ElementNodeConditionKind::If,
            prop: make_attr(0, 4),
        }));
        assert!(!b.set_v_for(make_attr(0, 5)));
        assert!(!b.set_v_slot(make_attr(0, 6)));
        assert!(!b.set_v_once(make_attr(0, 6)));
        assert!(!b.set_v_ref(make_attr(0, 3)));
        let ast = b.finish();
        assert!(ast.nodes.is_empty());
    }

    // ========================================================================
    // 31. HasChildWithVSlot propagation
    // ========================================================================

    /// @ai-generated - Tests that child with v_slot propagates HasChildWithVSlot to parent.
    #[test]
    fn children_flag_has_child_with_v_slot() {
        let mut b = TemplateAstBuilder::new(make_root());

        // <div>
        b.open_element(make_tag(0, 5, 4));
        b.mark_element_content_start(5);

        //   <template v-slot>
        b.open_element(make_tag(5, 22, 14));
        b.mark_element_content_start(22);
        b.close_element(Some(make_tag(22, 33, 32)), 22);

        // Set v_slot on child before closing parent
        let child_id = NodeId(1);
        if let AstNodeKind::Element(child_el) = &mut b.ast.nodes[child_id.0].kind {
            child_el.v_slot = Some(make_attr(15, 21));
        }

        // </div>
        b.close_element(Some(make_tag(33, 39, 38)), 33);

        let ast = b.finish();
        let div_id = ast.root.content.as_ref().unwrap().children[0];
        let AstNodeKind::Element(el) = &ast.nodes[div_id.0].kind else {
            panic!("expected Element");
        };

        assert!(el.children_flag.has(ChildrenFlags::HasChildWithVSlot));
        assert!(!el.children_flag.has(ChildrenFlags::HasDynamicSlotChild));
    }

    // ========================================================================
    // 32. HasDynamicSlotChild propagation
    // ========================================================================

    /// @ai-generated - Tests that child with dynamic v_slot propagates HasDynamicSlotChild.
    #[test]
    fn children_flag_has_dynamic_slot_child() {
        let mut b = TemplateAstBuilder::new(make_root());

        // <div>
        b.open_element(make_tag(0, 5, 4));
        b.mark_element_content_start(5);

        //   <template v-slot:[name]>
        b.open_element(make_tag(5, 28, 14));
        b.mark_element_content_start(28);
        b.close_element(Some(make_tag(28, 39, 38)), 28);

        // Set dynamic v_slot on child before closing parent
        let child_id = NodeId(1);
        if let AstNodeKind::Element(child_el) = &mut b.ast.nodes[child_id.0].kind {
            let mut slot_prop = make_attr(15, 21);
            slot_prop.is_dynamic = Some(true);
            child_el.v_slot = Some(slot_prop);
        }

        // </div>
        b.close_element(Some(make_tag(39, 45, 44)), 39);

        let ast = b.finish();
        let div_id = ast.root.content.as_ref().unwrap().children[0];
        let AstNodeKind::Element(el) = &ast.nodes[div_id.0].kind else {
            panic!("expected Element");
        };

        assert!(el.children_flag.has(ChildrenFlags::HasChildWithVSlot));
        assert!(el.children_flag.has(ChildrenFlags::HasDynamicSlotChild));
    }

    // ========================================================================
    // 33. HasChildWithKey propagation
    // ========================================================================

    /// @ai-generated - Tests that child with HasDynamicKey prop flag propagates HasChildWithKey.
    #[test]
    fn children_flag_has_child_with_key() {
        let mut b = TemplateAstBuilder::new(make_root());

        // <div>
        b.open_element(make_tag(0, 5, 4));
        b.mark_element_content_start(5);

        //   <span :key="id">
        b.open_element(make_tag(5, 22, 10));
        b.mark_element_content_start(22);
        b.close_element(Some(make_tag(22, 29, 28)), 22);

        // Set HasDynamicKey prop flag on child before closing parent
        let child_id = NodeId(1);
        if let AstNodeKind::Element(child_el) = &mut b.ast.nodes[child_id.0].kind {
            child_el.prop_flag = child_el.prop_flag.add(PropFlags::HasDynamicKey);
        }

        // </div>
        b.close_element(Some(make_tag(29, 35, 34)), 29);

        let ast = b.finish();
        let div_id = ast.root.content.as_ref().unwrap().children[0];
        let AstNodeKind::Element(el) = &ast.nodes[div_id.0].kind else {
            panic!("expected Element");
        };

        assert!(el.children_flag.has(ChildrenFlags::HasChildWithKey));
    }

    // ========================================================================
    // 34. Convenience method: parent() and children()
    // ========================================================================

    /// @ai-generated - Tests parent() and children() convenience methods.
    #[test]
    fn parent_and_children_accessors() {
        let mut b = TemplateAstBuilder::new(make_root());

        // <div>
        b.open_element(make_tag(0, 5, 4));
        b.mark_element_content_start(5);
        let text_id = b.add_text(5, 10, false);
        b.close_element(Some(make_tag(10, 16, 15)), 10);

        let ast = b.finish();
        let div_id = ast.root.content.as_ref().unwrap().children[0];

        // parent
        assert_eq!(ast.parent(div_id), None); // root child
        assert_eq!(ast.parent(text_id), Some(div_id)); // nested child

        // children
        let div_children = ast.children(div_id);
        assert_eq!(div_children.len(), 1);
        assert_eq!(div_children[0], text_id);

        // leaves have no children
        assert_eq!(ast.children(text_id).len(), 0);
    }
}
