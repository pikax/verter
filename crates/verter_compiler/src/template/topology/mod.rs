#![allow(dead_code)]

//! The Vue template's structure, built once from the parsed template.
//!
//! Dense node and region identities, parent/child/sibling/preorder tables,
//! and the control-flow regions (`v-if` branches, `v-for` bodies, slot
//! bodies, component children). No semantic fact and no target decision is
//! stored here. Identity is a dense index; source offsets live in a side
//! table.

mod build;
#[cfg(any(test, feature = "test-support"))]
mod dump;
mod ids;
mod tables;

pub use build::{build_template_topology, TopologyBuildError, TopologyRecoveryFacts};
#[cfg(any(test, feature = "test-support"))]
pub use dump::dump_topology;
pub use ids::{
    RegionCompleteness, SyntaxNodeId, VueBranchRole, VueIfBranchKey, VueIfChainId, VueIfChainKey,
    VueRegion, VueRegionId, VueRegionKind, VueTemplateNode, VueTemplateNodeId, VueTemplateNodeKey,
    VueTemplateNodeKind,
};
pub use tables::{VueForRegion, VueIfBranch, VueIfChain, VueSlotRegion, VueTemplateTopology};

#[cfg(test)]
mod tests;
