use verter_span::Span;

use super::{
    RegionCompleteness, SyntaxNodeId, VueBranchRole, VueIfBranchKey, VueIfChainId, VueIfChainKey,
    VueRegion, VueRegionId, VueRegionKind, VueTemplateNode, VueTemplateNodeId, VueTemplateNodeKey,
    VueTemplateNodeKind,
};
use crate::ast::types::{AstNodeKind, ElementNode, TemplateAst};
use crate::framework_common::structure::{Arena, FlatArena, FlatRange, Frontend, TopologyStore};

/// One branch of a `v-if` chain, in source order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VueIfBranch {
    pub region: VueRegionId,
    pub node: VueTemplateNodeId,
    pub role: VueBranchRole,
}

/// One `v-if` chain among the children of `owner`.
#[derive(Clone, Copy)]
pub struct VueIfChain {
    pub owner: VueTemplateNodeId,
    pub(super) branches: FlatRange<VueIfBranchKey>,
}

/// A `v-for` body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VueForRegion {
    pub region: VueRegionId,
    pub node: VueTemplateNodeId,
}

/// One slot body of a component.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VueSlotRegion {
    pub region: VueRegionId,
    pub component: VueTemplateNodeId,
    /// The `<template v-slot>` node; `None` for a slot declared on the
    /// component tag and for the implicit default slot.
    pub template: Option<VueTemplateNodeId>,
    pub implicit_default: bool,
}

/// The template's topology. Borrows the source and the parsed template.
pub struct VueTemplateTopology<'p> {
    pub(super) ast: &'p TemplateAst,
    pub(super) store: TopologyStore<'p, VueTemplateNodeKey, VueRegion, VueTemplateNode>,
    /// Parser index to topology node; `None` for nodes the parser dropped.
    pub(super) by_syntax: Vec<Option<VueTemplateNodeId>>,
    pub(super) root: VueTemplateNodeId,
    pub(super) root_region: VueRegionId,
    pub(super) if_chains: Arena<VueIfChainKey, VueIfChain, Frontend>,
    pub(super) if_branches: FlatArena<VueIfBranchKey, VueIfBranch, Frontend>,
    /// Sorted by node index.
    pub(super) for_regions: Vec<VueForRegion>,
    /// Sorted by component index, then source order.
    pub(super) slot_regions: Vec<VueSlotRegion>,
    /// Sorted by branch node index: node, chain, index in chain.
    pub(super) chain_of: Vec<(VueTemplateNodeId, VueIfChainId, u32)>,
    pub(super) completeness: RegionCompleteness,
}

impl<'p> VueTemplateTopology<'p> {
    // ── identity and payload ────────────────────────────────────────
    pub fn len(&self) -> usize {
        self.store.len()
    }
    pub fn is_empty(&self) -> bool {
        self.store.is_empty()
    }
    /// The synthetic root node.
    pub fn root(&self) -> VueTemplateNodeId {
        self.root
    }
    pub fn root_region(&self) -> VueRegionId {
        self.root_region
    }
    /// The parsed template this topology was built from.
    pub fn ast(&self) -> &'p TemplateAst {
        self.ast
    }
    /// Every node in preorder.
    pub fn nodes(&self) -> impl Iterator<Item = VueTemplateNodeId> + '_ {
        self.store.node_ids()
    }
    pub fn node(&self, id: VueTemplateNodeId) -> VueTemplateNode {
        *self
            .store
            .node(id)
            .expect("node id belongs to this topology")
    }
    pub fn kind(&self, id: VueTemplateNodeId) -> VueTemplateNodeKind {
        self.node(id).kind
    }
    /// Authored span of the node (side table; never identity).
    pub fn span(&self, id: VueTemplateNodeId) -> Span {
        self.store
            .span(id)
            .expect("node id belongs to this topology")
    }
    pub fn syntax(&self, id: VueTemplateNodeId) -> Option<SyntaxNodeId> {
        let syntax = self.node(id).syntax;
        (syntax != u32::MAX).then_some(crate::types::NodeId(syntax as usize))
    }
    pub fn node_for_syntax(&self, syntax: SyntaxNodeId) -> Option<VueTemplateNodeId> {
        self.by_syntax.get(syntax.0).copied().flatten()
    }
    /// The source retained by the topology's canonical storage.
    pub(crate) fn source(&self) -> &'p str {
        self.store.source()
    }
    /// The parser's element payload, for the four element-like kinds.
    pub fn element(&self, id: VueTemplateNodeId) -> Option<&'p ElementNode> {
        let syntax = self.syntax(id)?;
        match &self.ast.nodes.get(syntax.0)?.kind {
            AstNodeKind::Element(element) => Some(element),
            _ => None,
        }
    }

    // ── tree ────────────────────────────────────────────────────────
    pub fn parent(&self, id: VueTemplateNodeId) -> Option<VueTemplateNodeId> {
        self.store.parent(id)
    }
    pub fn children(&self, id: VueTemplateNodeId) -> impl Iterator<Item = VueTemplateNodeId> + '_ {
        self.store.children(id)
    }
    pub fn next_sibling(&self, id: VueTemplateNodeId) -> Option<VueTemplateNodeId> {
        self.store.next_sibling(id)
    }
    /// The node and its descendants, in preorder.
    pub fn subtree(&self, id: VueTemplateNodeId) -> impl Iterator<Item = VueTemplateNodeId> + '_ {
        self.store.subtree(id).into_iter().flatten()
    }

    // ── regions ─────────────────────────────────────────────────────
    /// Innermost region containing the node.
    pub fn region_of(&self, id: VueTemplateNodeId) -> VueRegionId {
        self.store
            .region(id)
            .expect("node id belongs to this topology")
    }
    pub fn regions(&self) -> impl Iterator<Item = VueRegionId> + '_ {
        self.store.region_ids()
    }
    pub fn region(&self, id: VueRegionId) -> VueRegion {
        *self
            .store
            .region_data(id)
            .expect("region id belongs to this topology")
    }
    pub fn region_kind(&self, id: VueRegionId) -> VueRegionKind {
        self.region(id).kind
    }
    pub fn region_parent(&self, id: VueRegionId) -> Option<VueRegionId> {
        self.store.region_parent(id)
    }
    /// The node immediately outside the region's body.
    pub fn region_owner(&self, id: VueRegionId) -> Option<VueTemplateNodeId> {
        self.store.region_owner(id)
    }
    /// Regions from the node's innermost region out to the root region.
    pub fn enclosing_regions(
        &self,
        id: VueTemplateNodeId,
    ) -> impl Iterator<Item = VueRegionId> + '_ {
        std::iter::successors(Some(self.region_of(id)), |&region| {
            self.region_parent(region)
        })
    }
    /// The least complete region marking in the template.
    pub fn completeness(&self) -> RegionCompleteness {
        self.completeness
    }

    // ── control flow ────────────────────────────────────────────────
    pub fn if_chains(&self) -> impl Iterator<Item = VueIfChainId> + '_ {
        self.if_chains.ids()
    }
    pub fn if_chain(&self, id: VueIfChainId) -> VueIfChain {
        *self
            .if_chains
            .get(id)
            .expect("chain id belongs to this topology")
    }
    /// Branches of a chain in source order.
    pub fn branches(&self, id: VueIfChainId) -> &[VueIfBranch] {
        self.if_branches.get(self.if_chain(id).branches)
    }
    /// The chain a branch node belongs to, and its index in that chain.
    pub fn chain_of(&self, node: VueTemplateNodeId) -> Option<(VueIfChainId, u32)> {
        let at = self
            .chain_of
            .binary_search_by_key(&node.index(), |row| row.0.index())
            .ok()?;
        let (_, chain, index) = self.chain_of[at];
        Some((chain, index))
    }
    pub fn for_region(&self, node: VueTemplateNodeId) -> Option<VueForRegion> {
        let at = self
            .for_regions
            .binary_search_by_key(&node.index(), |row| row.node.index())
            .ok()?;
        Some(self.for_regions[at])
    }
    pub fn for_regions(&self) -> &[VueForRegion] {
        &self.for_regions
    }
    /// Slot bodies of a component, in source order.
    pub fn slot_regions(&self, component: VueTemplateNodeId) -> &[VueSlotRegion] {
        let start = self
            .slot_regions
            .partition_point(|row| row.component.index() < component.index());
        let len = self.slot_regions[start..]
            .iter()
            .take_while(|row| row.component == component)
            .count();
        &self.slot_regions[start..start + len]
    }
}
