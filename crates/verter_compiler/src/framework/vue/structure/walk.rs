use super::{VueCompileNodeId, VueCompileStructure};
use crate::template::topology::{VueTemplateNodeId, VueTemplateNodeKind, VueTemplateTopology};

/// Whether to descend into a node's children.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VueWalkControl {
    Continue,
    SkipChildren,
}

/// A typed visit of the template. Implemented by each target; the walk is
/// monomorphized per visitor, so no call here is dynamically dispatched.
pub trait VueTemplateVisitor {
    fn enter_template(&mut self);
    fn leave_template(&mut self);
    /// Element, component, slot outlet or `<template>`.
    fn enter_element(
        &mut self,
        node: VueCompileNodeId,
        kind: VueTemplateNodeKind,
    ) -> VueWalkControl;
    /// Called for every entered element, after its children.
    fn leave_element(&mut self, node: VueCompileNodeId, kind: VueTemplateNodeKind);
    fn visit_text(&mut self, node: VueCompileNodeId);
    fn visit_interpolation(&mut self, node: VueCompileNodeId);
    fn visit_comment(&mut self, node: VueCompileNodeId);
}

/// Depth-first walk in source order with an explicit stack. Every child of
/// a node is visited; a visitor that wants the resolved children reads
/// `structure.children` itself.
pub fn walk_template<V: VueTemplateVisitor>(
    topology: &VueTemplateTopology<'_>,
    structure: &VueCompileStructure,
    visitor: &mut V,
) {
    visitor.enter_template();
    let mut open: Vec<VueTemplateNodeId> = Vec::new();
    let mut cursor = topology.children(topology.root()).next();
    loop {
        let Some(node) = cursor else {
            let Some(parent) = open.pop() else { break };
            visitor.leave_element(structure.compile_id(parent), topology.kind(parent));
            cursor = topology.next_sibling(parent);
            continue;
        };
        let id = structure.compile_id(node);
        let kind = topology.kind(node);
        match kind {
            VueTemplateNodeKind::Root => {}
            VueTemplateNodeKind::Text => visitor.visit_text(id),
            VueTemplateNodeKind::Interpolation => visitor.visit_interpolation(id),
            VueTemplateNodeKind::Comment => visitor.visit_comment(id),
            VueTemplateNodeKind::Element
            | VueTemplateNodeKind::Component
            | VueTemplateNodeKind::SlotOutlet
            | VueTemplateNodeKind::Template => {
                let control = visitor.enter_element(id, kind);
                let first = match control {
                    VueWalkControl::Continue => topology.children(node).next(),
                    VueWalkControl::SkipChildren => None,
                };
                if let Some(first) = first {
                    open.push(node);
                    cursor = Some(first);
                    continue;
                }
                visitor.leave_element(id, kind);
            }
        }
        cursor = topology.next_sibling(node);
    }
    visitor.leave_template();
}
