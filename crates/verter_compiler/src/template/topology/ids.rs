use crate::framework_common::structure::{Id, NodeId, RegionId};

/// The parser's node identity (index into `TemplateAst::nodes`).
pub type SyntaxNodeId = crate::types::NodeId;

/// Collection marker of topology nodes.
pub enum VueTemplateNodeKey {}
/// Dense preorder identity of a template node. Snapshot-local.
pub type VueTemplateNodeId = NodeId<VueTemplateNodeKey>;
/// Dense identity of a control-flow region. Snapshot-local.
pub type VueRegionId = RegionId<VueRegion>;

/// Collection marker of `v-if` chains.
pub enum VueIfChainKey {}
pub type VueIfChainId = Id<VueIfChainKey>;
/// Collection marker of `v-if` chain branches.
pub enum VueIfBranchKey {}

/// Syntactic node kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum VueTemplateNodeKind {
    /// The synthetic root that owns the template's top-level children.
    Root,
    Element,
    Component,
    SlotOutlet,
    Template,
    Text,
    Interpolation,
    Comment,
}

/// Compact node record. 8 bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VueTemplateNode {
    pub kind: VueTemplateNodeKind,
    /// Parser arena index; `u32::MAX` for the synthetic root.
    pub(super) syntax: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum VueRegionKind {
    /// The whole template.
    Root,
    /// One branch of a `v-if` / `v-else-if` / `v-else` chain. Contains the
    /// branch node and its subtree.
    IfBranch,
    /// The repeated subtree of a `v-for`. Contains the node carrying it.
    ForBody,
    /// The content of one slot: the children of a `<template v-slot>`, the
    /// children of a component carrying `v-slot` itself, or a component's
    /// loose children (the implicit default slot).
    SlotBody,
    /// The direct children of a component that are slot templates.
    ComponentChildren,
}

/// Whether a region's markup is trustworthy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum RegionCompleteness {
    /// Parsed with no recovery inside the region.
    Complete,
    /// The parser recovered inside the region; structure is usable by
    /// tolerant consumers only.
    Recovered,
    /// Structure inside the region is missing or cut off.
    Incomplete,
}

/// Compact region record. 8 bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VueRegion {
    pub kind: VueRegionKind,
    pub completeness: RegionCompleteness,
    /// Index into the kind's own table: the chain for `IfBranch`, the loop
    /// table for `ForBody`, the slot table for `SlotBody`; `u32::MAX` for
    /// `Root` and `ComponentChildren`, which have none.
    pub(super) detail: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum VueBranchRole {
    If,
    ElseIf,
    Else,
}

const _: () = {
    assert!(std::mem::size_of::<VueTemplateNode>() == 8);
    assert!(std::mem::size_of::<VueRegion>() == 8);
};
