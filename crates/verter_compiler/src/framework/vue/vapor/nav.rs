//! Shared rendered-child facts and deferred reference reservation.
use super::effect::VaporEffect;
use super::ops::{ClosedBlock, InsertionAnchor, NameKind, Nav, Part, Stmt};
use super::scope::{MergedConstructKind, PendingNavRequest, VaporScope};
use super::VaporCompiler;
use crate::framework::vue::structure::{VueChainRole, VueChildKind, VueCompileNodeId};
use crate::framework::vue::target::{VueTargetInputs, VueTargetRefusal};
use crate::template::topology::VueTemplateNodeKind;

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct ChildInfo {
    pub(super) kind: Option<VueChildKind>,
    /// An `else`/`else-if` branch continuing the preceding conditional chain.
    pub(super) continuation: bool,
    pub(super) following_template: bool,
    pub(super) sole_meaningful: bool,
    /// Every rendered child is text or interpolation, so the node's
    /// children are one text run its first child holds.
    pub(super) children_all_text_like: bool,
    /// The block roots the node's children make as a slot body: a run of
    /// text is one root, every other rendered child (an element, a
    /// construct, a kept comment) is one, a `<template v-slot>` is its own
    /// slot and a `v-else` branch belongs to its chain.
    pub(super) slot_body_roots: u32,
}

/// One linear projection of the structure's already-resolved child lists.
pub(super) fn child_info(inputs: &VueTargetInputs<'_>) -> Vec<ChildInfo> {
    let mut rows = vec![ChildInfo::default(); inputs.structure.len()];
    for parent in inputs.topology.nodes() {
        let parent_id = inputs.structure.compile_id(parent);
        let children = inputs.structure.children(parent_id);
        // A `<template v-slot>` is its own slot function, not a node of the
        // component's default slot content.
        let slot_template = |node: VueCompileNodeId| {
            let template = inputs.structure.template_id(node);
            inputs.topology.kind(template) == VueTemplateNodeKind::Template
                && inputs
                    .topology
                    .element(template)
                    .is_some_and(|element| element.v_slot.is_some())
        };
        let meaningful = children
            .iter()
            .filter(|child| {
                !matches!(
                    child.kind,
                    VueChildKind::WhitespaceSpace | VueChildKind::WhitespaceNewline
                ) && !slot_template(child.node)
            })
            .count();
        let mut following_template = false;
        let mut all_text_like = true;
        let mut slot_body_roots = 0;
        let mut in_text_run = false;
        for child in children {
            if slot_template(child.node) || child.chain == VueChainRole::Continuation {
                continue;
            }
            match child.kind {
                VueChildKind::Text
                | VueChildKind::Interpolation
                | VueChildKind::WhitespaceSpace
                | VueChildKind::WhitespaceNewline => {
                    if !in_text_run {
                        slot_body_roots += 1;
                        in_text_run = true;
                    }
                }
                VueChildKind::Comment if !inputs.options.comments => {}
                VueChildKind::Element | VueChildKind::Comment => {
                    slot_body_roots += 1;
                    in_text_run = false;
                }
            }
        }
        for child in children.iter().rev() {
            let template = inputs.structure.template_id(child.node);
            let contributes = match inputs.topology.kind(template) {
                VueTemplateNodeKind::Element => {
                    inputs.topology.element(template).is_some_and(|element| {
                        element.v_condition.is_none() && element.v_for.is_none()
                    })
                }
                VueTemplateNodeKind::Text
                | VueTemplateNodeKind::Interpolation
                | VueTemplateNodeKind::Comment => true,
                _ => false,
            };
            let whitespace = matches!(
                child.kind,
                VueChildKind::WhitespaceSpace | VueChildKind::WhitespaceNewline
            );
            let row = &mut rows[child.node.index()];
            row.kind = Some(child.kind);
            row.continuation = child.chain == VueChainRole::Continuation;
            row.following_template = following_template;
            if slot_template(child.node) {
                continue;
            }
            row.sole_meaningful = meaningful == 1 && !whitespace;
            // A continuation occupies its source position but is part of the
            // preceding construct. Whitespace the structure keeps is a text
            // node of the template, so a block before it needs an anchor.
            if contributes && child.chain != VueChainRole::Continuation {
                following_template = true;
            }
            // Any other rendered child (an element, a construct, a kept
            // comment) is a node beside the text: each text run is then
            // reached as its own node, never as the parent's first child.
            let text_like = match child.kind {
                VueChildKind::Text | VueChildKind::Interpolation => true,
                VueChildKind::Comment => !inputs.options.comments,
                _ => whitespace,
            };
            if !text_like {
                all_text_like = false;
            }
        }
        rows[parent_id.index()].children_all_text_like = all_text_like;
        rows[parent_id.index()].slot_body_roots = slot_body_roots;
    }
    rows
}

impl<'c> VaporCompiler<'c> {
    pub(super) fn has_following_template_contributing_sibling(
        &self,
        node: VueCompileNodeId,
    ) -> bool {
        self.child_info[node.index()].following_template
    }

    pub(super) fn is_sole_meaningful_child(&self, node: VueCompileNodeId) -> bool {
        self.child_info[node.index()].sole_meaningful
    }

    pub(super) fn take_scope_html(&mut self, scope: &mut VaporScope<'c>) {
        let enclosing = self.html_scope_stack.pop().unwrap_or_default();
        scope.html = std::mem::replace(&mut self.html, enclosing);
    }

    pub(super) fn resolve_pending_nav_requests(
        &mut self,
        scope: &mut VaporScope<'c>,
    ) -> Result<(), VueTargetRefusal> {
        // The last text run ends with its scope.
        scope.end_text_run();
        let requests = std::mem::take(&mut scope.pending_nav_requests);
        for request in requests {
            match request {
                PendingNavRequest::Merge {
                    dom_child_index,
                    has_following,
                    nav_slot,
                    stmt_slot,
                } => {
                    let anchor = if has_following {
                        // The anchor id precedes the memoized container reference.
                        let anchor = self.state.counters.next_node();
                        let container = scope.ensure_node_ref(&mut self.state.counters);
                        scope.child_nav[nav_slot.expect("anchor reserves navigation")] = Nav::At {
                            target: (NameKind::Node, anchor),
                            container,
                            index: dom_child_index,
                            text: false,
                        };
                        InsertionAnchor::NodeRef(anchor)
                    } else if dom_child_index == 0 {
                        InsertionAnchor::Append
                    } else {
                        InsertionAnchor::Index(dom_child_index)
                    };
                    let container = scope.ensure_node_ref(&mut self.state.counters);
                    scope.child_statements[stmt_slot] = Stmt::InsertionState { container, anchor };
                }
                PendingNavRequest::OwnRef {
                    own_ref,
                    dom_child_index,
                    nav_slot,
                } => {
                    let container = scope.ensure_node_ref(&mut self.state.counters);
                    scope.child_nav[nav_slot] = Nav::At {
                        target: (NameKind::Node, own_ref),
                        container,
                        index: dom_child_index,
                        text: false,
                    };
                }
                PendingNavRequest::TextRef {
                    own_ref,
                    dom_child_index,
                    nav_slot,
                    stmt_slot,
                    parts,
                } => {
                    let container = scope.ensure_node_ref(&mut self.state.counters);
                    scope.child_nav[nav_slot] = Nav::At {
                        target: (NameKind::Node, own_ref),
                        container,
                        index: dom_child_index,
                        text: true,
                    };
                    let effect = self.state.add_effect(VaporEffect::Text {
                        text_ref: own_ref,
                        parts,
                        generated: false,
                    });
                    let group = self.state.add_effect_group(vec![effect], "  ", true)?;
                    let run = self.state.runs.commit(&mut vec![Part::Effects(group)]);
                    scope.child_statements[stmt_slot] = Stmt::Run(run);
                }
            }
        }
        Ok(())
    }

    pub(super) fn bubble_structural_content_into_parent(
        &mut self,
        mut scope: VaporScope<'c>,
        dom_child_index: u32,
        already_navigated: bool,
    ) -> VaporScope<'c> {
        if scope.node_ref.is_none() && scope.child_statements.is_empty() {
            return scope;
        }
        if let Some(parent) = self.element_stack.last_mut() {
            if !already_navigated {
                if let Some(own_ref) = scope.node_ref {
                    let nav_slot = parent.child_nav.len();
                    parent.child_nav.push(Nav::Reserved);
                    parent.pending_nav_requests.push(PendingNavRequest::OwnRef {
                        own_ref,
                        dom_child_index,
                        nav_slot,
                    });
                }
            }
            parent.child_nav.append(&mut scope.child_nav);
            parent
                .child_text_creations
                .append(&mut scope.child_text_creations);
            parent.child_effects.append(&mut scope.child_effects);
            parent.child_statements.append(&mut scope.child_statements);
        }
        scope
    }

    pub(super) fn merge_into_stack_index(
        &mut self,
        index: usize,
        node: VueCompileNodeId,
        root: ClosedBlock,
        kind: MergedConstructKind,
    ) {
        let has_following = self.has_following_template_contributing_sibling(node);
        if self
            .element_stack
            .get(index)
            .is_some_and(|parent| parent.is_transparent_wrapper)
        {
            // A wrapper has no DOM container: the construct is one of its roots.
            let run = self.state.runs.commit(&mut vec![
                Part::Text("    "),
                Part::Indented(root.run, "    "),
                Part::Text("\n"),
            ]);
            self.element_stack[index]
                .roots
                .push(ClosedBlock { run, ..root });
            return;
        }
        let Some(parent) = self.element_stack.get_mut(index) else {
            return;
        };
        let dom_child_index = parent.observe_dom_element();
        parent.merged_construct_kinds.push((kind, root.node_ref));
        let nav_slot = if has_following {
            self.html.push_str("<!>");
            let slot = parent.child_nav.len();
            parent.child_nav.push(Nav::Reserved);
            Some(slot)
        } else {
            None
        };
        let stmt_slot = parent.child_statements.len();
        parent.child_statements.push(Stmt::Reserved);
        parent.pending_nav_requests.push(PendingNavRequest::Merge {
            dom_child_index,
            has_following,
            nav_slot,
            stmt_slot,
        });
        parent.child_statements.push(Stmt::Run(root.run));
    }
}
