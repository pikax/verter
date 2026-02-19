//! Arena-based template AST with O(1) tree navigation.
//!
//! Nodes are allocated in a flat `Vec<AstNode>` arena indexed by [`NodeId`].
//! Each node stores its parent and its index within the parent's children,
//! enabling O(1) `parent()`, `children()`, `next_sibling()`, and
//! `prev_sibling()` lookups without pointer chasing.
//!
//! The [`builder`] sub-module provides [`TemplateAstBuilder`] for incremental
//! construction from tokenizer events.

use smallvec::SmallVec;

use crate::new_impl::{
    ast::types::{AstNode, AstNodeKind, ElementContent, TemplateAst},
    syntax::types::{RootNodeTemplate, RootNodeTemplateContent},
    types::NodeId,
};

pub mod builder;
pub mod types;

impl TemplateAst {
    pub fn new(root: RootNodeTemplate) -> Self {
        Self {
            nodes: Vec::new(),
            root,
        }
    }

    /// Allocate a node in the arena and return its NodeId.
    /// NOTE: parent/index_in_parent are filled when you ATTACH the node.
    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    pub fn alloc_node(&mut self, kind: AstNodeKind) -> NodeId {
        let id = NodeId(self.nodes.len());
        self.nodes.push(AstNode {
            kind,
            parent: None,
            index_in_parent: 0,
        });
        id
    }

    fn root_children_mut(&mut self) -> &mut RootNodeTemplateContent {
        self.root
            .content
            .get_or_insert_with(|| RootNodeTemplateContent {
                start: 0,
                end: 0,
                children: SmallVec::new(),
            })
    }

    fn root_children(&self) -> &[NodeId] {
        self.root
            .content
            .as_ref()
            .map(|c| c.children.as_slice())
            .unwrap_or(&[])
    }

    /// Attach an existing node as a root-level child.
    pub fn attach_to_root(&mut self, child: NodeId) {
        // Take the mutable root content once.
        let root_content = self.root_children_mut();

        // Compute index and push.
        let idx = root_content.children.len();
        root_content.children.push(child);

        // Now update the child node.
        let n = &mut self.nodes[child.0];
        n.parent = None;
        n.index_in_parent = idx;
    }

    /// Attach an existing node as a child of an existing element node.
    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    pub fn attach_to_parent(&mut self, parent: NodeId, child: NodeId) {
        // Ensure parent is an Element and has content.
        let idx = {
            let p = &mut self.nodes[parent.0];
            let AstNodeKind::Element(el) = &mut p.kind else {
                panic!("attach_to_parent: parent is not an Element");
            };
            let content = el.content.get_or_insert_with(|| ElementContent {
                start: 0,
                end: 0,
                children: SmallVec::new(),
            });
            let idx = content.children.len();
            content.children.push(child);
            idx
        };

        {
            let c = &mut self.nodes[child.0];
            c.parent = Some(parent);
            c.index_in_parent = idx;
        }
    }

    /// O(1) access to the siblings slice.
    pub fn siblings(&self, id: NodeId) -> &[NodeId] {
        let node = &self.nodes[id.0];
        if let Some(parent_id) = node.parent {
            let parent = &self.nodes[parent_id.0];
            match &parent.kind {
                AstNodeKind::Element(el) => {
                    debug_assert!(
                        el.content.is_some(),
                        "parent element has no content but is used for siblings()"
                    );

                    el.content
                        .as_ref()
                        .map(|c| c.children.as_slice())
                        .unwrap_or(&[])
                }
                _ => {
                    // By construction, only elements can be parents (since leaves can't have children).
                    &[]
                }
            }
        } else {
            self.root_children()
        }
    }

    /// O(1) next sibling.
    pub fn next_sibling(&self, id: NodeId) -> Option<NodeId> {
        let node = &self.nodes[id.0];
        self.siblings(id).get(node.index_in_parent + 1).copied()
    }

    /// O(1) previous sibling.
    pub fn prev_sibling(&self, id: NodeId) -> Option<NodeId> {
        let node = &self.nodes[id.0];
        if node.index_in_parent == 0 {
            None
        } else {
            self.siblings(id).get(node.index_in_parent - 1).copied()
        }
    }

    /// O(1) parent lookup.
    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.nodes[id.0].parent
    }

    /// O(1) children of an element node (empty slice for leaves).
    pub fn children(&self, id: NodeId) -> &[NodeId] {
        match &self.nodes[id.0].kind {
            AstNodeKind::Element(el) => el
                .content
                .as_ref()
                .map(|c| c.children.as_slice())
                .unwrap_or(&[]),
            _ => &[],
        }
    }

    /// Optional: iterative DFS traversal.
    pub fn dfs(&self, start: NodeId, mut f: impl FnMut(NodeId, &AstNode)) {
        let mut stack = vec![start];
        while let Some(id) = stack.pop() {
            let node = &self.nodes[id.0];
            f(id, node);

            if let AstNodeKind::Element(el) = &node.kind {
                if let Some(content) = &el.content {
                    for &child in content.children.iter().rev() {
                        stack.push(child);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod ast_tests {
    use super::*;
    use crate::new_impl::ast::builder::TemplateAstBuilder;
    use crate::new_impl::ast::types::{AstNodeKind, TextNode};
    use crate::new_impl::test_helpers::{make_root, make_tag};

    /// Build a simple tree: root → [div → [a, b, c]]
    fn build_tree_with_three_siblings() -> TemplateAst {
        let mut b = TemplateAstBuilder::new(make_root());

        // <div>
        b.open_element(make_tag(0, 5, 4));
        b.mark_element_content_start(5);
        b.add_text(5, 6, false); // a
        b.add_text(6, 7, false); // b
        b.add_text(7, 8, false); // c
                                 // </div>
        b.close_element(Some(make_tag(8, 14, 13)), 8);

        b.finish()
    }

    // ========================================================================
    // siblings
    // ========================================================================

    /// @ai-generated - Tests siblings of a root child returns root children.
    #[test]
    fn siblings_of_root_child() {
        let mut b = TemplateAstBuilder::new(make_root());
        b.add_text(0, 5, false);
        b.add_text(5, 10, false);
        let ast = b.finish();

        let first = ast.root.content.as_ref().unwrap().children[0];
        let sibs = ast.siblings(first);
        assert_eq!(sibs.len(), 2);
    }

    /// @ai-generated - Tests siblings of a nested child returns parent's children.
    #[test]
    fn siblings_of_nested_child() {
        let ast = build_tree_with_three_siblings();

        let div_id = ast.root.content.as_ref().unwrap().children[0];
        let AstNodeKind::Element(div) = &ast.nodes[div_id.0].kind else {
            panic!("expected Element");
        };
        let first_child = div.content.as_ref().unwrap().children[0];
        let sibs = ast.siblings(first_child);
        assert_eq!(sibs.len(), 3);
    }

    // ========================================================================
    // next_sibling / prev_sibling
    // ========================================================================

    /// @ai-generated - Tests next_sibling traversal.
    #[test]
    fn next_sibling_traversal() {
        let ast = build_tree_with_three_siblings();

        let div_id = ast.root.content.as_ref().unwrap().children[0];
        let AstNodeKind::Element(div) = &ast.nodes[div_id.0].kind else {
            panic!("expected Element");
        };
        let children = &div.content.as_ref().unwrap().children;
        let a = children[0];
        let b_id = children[1];
        let c = children[2];

        assert_eq!(ast.next_sibling(a), Some(b_id));
        assert_eq!(ast.next_sibling(b_id), Some(c));
        assert_eq!(ast.next_sibling(c), None);
    }

    /// @ai-generated - Tests prev_sibling traversal.
    #[test]
    fn prev_sibling_traversal() {
        let ast = build_tree_with_three_siblings();

        let div_id = ast.root.content.as_ref().unwrap().children[0];
        let AstNodeKind::Element(div) = &ast.nodes[div_id.0].kind else {
            panic!("expected Element");
        };
        let children = &div.content.as_ref().unwrap().children;
        let a = children[0];
        let b_id = children[1];
        let c = children[2];

        assert_eq!(ast.prev_sibling(a), None);
        assert_eq!(ast.prev_sibling(b_id), Some(a));
        assert_eq!(ast.prev_sibling(c), Some(b_id));
    }

    /// @ai-generated - Tests sibling navigation for root-level children.
    #[test]
    fn root_sibling_navigation() {
        let mut b = TemplateAstBuilder::new(make_root());
        b.add_text(0, 3, false);
        b.add_text(3, 6, false);
        b.add_text(6, 9, false);
        let ast = b.finish();

        let children = &ast.root.content.as_ref().unwrap().children;
        let first = children[0];
        let second = children[1];
        let third = children[2];

        assert_eq!(ast.prev_sibling(first), None);
        assert_eq!(ast.next_sibling(first), Some(second));
        assert_eq!(ast.next_sibling(second), Some(third));
        assert_eq!(ast.next_sibling(third), None);
    }

    // ========================================================================
    // dfs traversal
    // ========================================================================

    /// @ai-generated - Tests DFS traversal visits nodes in correct order.
    #[test]
    fn dfs_traversal_order() {
        let mut b = TemplateAstBuilder::new(make_root());

        // <div>
        b.open_element(make_tag(0, 5, 4)); // node 0: div
        b.mark_element_content_start(5);
        b.add_text(5, 6, false); // node 1: text "a"
                                 // <span>
        b.open_element(make_tag(6, 12, 11)); // node 2: span
        b.mark_element_content_start(12);
        b.add_text(12, 13, false); // node 3: text "b"
                                   // </span>
        b.close_element(Some(make_tag(13, 20, 19)), 13);
        b.add_text(20, 21, false); // node 4: text "c"
                                   // </div>
        b.close_element(Some(make_tag(21, 27, 26)), 21);

        let ast = b.finish();

        let div_id = ast.root.content.as_ref().unwrap().children[0];
        let mut visited = Vec::new();
        ast.dfs(div_id, |id, _node| {
            visited.push(id);
        });

        // DFS order: div → text_a → span → text_b → text_c
        assert_eq!(visited.len(), 5);
        assert_eq!(visited[0], div_id); // div
                                        // text_a, span, text_b, text_c
        assert!(matches!(
            ast.nodes[visited[1].0].kind,
            AstNodeKind::Text(TextNode { start: 5, .. })
        ));
        assert!(matches!(
            ast.nodes[visited[2].0].kind,
            AstNodeKind::Element(_)
        )); // span
        assert!(matches!(
            ast.nodes[visited[3].0].kind,
            AstNodeKind::Text(TextNode { start: 12, .. })
        ));
        assert!(matches!(
            ast.nodes[visited[4].0].kind,
            AstNodeKind::Text(TextNode { start: 20, .. })
        ));
    }

    /// @ai-generated - Tests DFS on a leaf node visits only that node.
    #[test]
    fn dfs_leaf_node() {
        let mut b = TemplateAstBuilder::new(make_root());
        let text_id = b.add_text(0, 5, false);
        let ast = b.finish();

        let mut visited = Vec::new();
        ast.dfs(text_id, |id, _| visited.push(id));

        assert_eq!(visited.len(), 1);
        assert_eq!(visited[0], text_id);
    }

    // ========================================================================
    // alloc_node / attach
    // ========================================================================

    /// @ai-generated - Tests alloc_node assigns sequential NodeIds.
    #[test]
    fn alloc_node_sequential_ids() {
        let mut ast = TemplateAst::new(make_root());

        let id0 = ast.alloc_node(AstNodeKind::Text(TextNode {
            start: 0,
            end: 1,
            is_entity: false,
        }));
        let id1 = ast.alloc_node(AstNodeKind::Text(TextNode {
            start: 1,
            end: 2,
            is_entity: false,
        }));

        assert_eq!(id0, NodeId(0));
        assert_eq!(id1, NodeId(1));
        assert_eq!(ast.nodes.len(), 2);
    }

    /// @ai-generated - Tests attach_to_root sets parent=None and correct index.
    #[test]
    fn attach_to_root_sets_metadata() {
        let mut ast = TemplateAst::new(make_root());

        let id0 = ast.alloc_node(AstNodeKind::Text(TextNode {
            start: 0,
            end: 1,
            is_entity: false,
        }));
        let id1 = ast.alloc_node(AstNodeKind::Text(TextNode {
            start: 1,
            end: 2,
            is_entity: false,
        }));

        ast.attach_to_root(id0);
        ast.attach_to_root(id1);

        assert!(ast.nodes[id0.0].parent.is_none());
        assert_eq!(ast.nodes[id0.0].index_in_parent, 0);
        assert!(ast.nodes[id1.0].parent.is_none());
        assert_eq!(ast.nodes[id1.0].index_in_parent, 1);

        let root_children = ast.root.content.as_ref().unwrap();
        assert_eq!(root_children.children.len(), 2);
    }
}
