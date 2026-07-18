//! Small plan accessors and operation-dedup state shared by the client planner.

use super::client_plan::{ClientModulePlan, ClientRuntimeOp};
use super::ir::NodeId;

impl<'a> ClientModulePlan<'a> {
    /// The reactive ops owned by a specific template-scope region (empty when the region
    /// has no ops). The emitter reads this per region so a block body's effect is built
    /// from the body's ops, never the root's.
    pub(super) fn ops_in(&self, scope: super::ir::TemplateScopeId) -> &[ClientRuntimeOp] {
        self.region_ops
            .iter()
            .find(|r| r.scope_id == scope)
            .map_or(&[], |r| r.ops.as_slice())
    }

    /// Every reactive op across all regions, in region-then-source order. Used by the
    /// by-unique-target lookups (a node lives in one region, so a flat scan resolves it).
    pub(super) fn all_ops(&self) -> impl Iterator<Item = &ClientRuntimeOp> {
        self.region_ops.iter().flat_map(|r| r.ops.iter())
    }
}

/// The per-element first-op dedup state threaded through `project_scope_op` (one
/// coalesced `$.set_class` / `$.set_style` / `$.set_attribute` / `$.attribute_effect`
/// per element). Global across regions — an element lives in exactly one region.
#[derive(Default)]
pub(super) struct OpDedup {
    /// Targets whose coalesced `$.set_class` has been emitted.
    pub(super) class_done: rustc_hash::FxHashSet<NodeId>,
    /// Targets whose coalesced `$.set_style` has been emitted.
    pub(super) style_done: rustc_hash::FxHashSet<NodeId>,
    /// `(target, attr-name)` pairs whose whole plain-attribute value has been emitted.
    pub(super) plain_attr_done: rustc_hash::FxHashSet<(NodeId, String)>,
    /// Spread elements whose `$.attribute_effect` fold has been emitted.
    pub(super) spread_attrs_done: rustc_hash::FxHashSet<NodeId>,
}
