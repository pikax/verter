//! The per-function [`FunctionFlowGraph`]: a sparse, arena-free typed-edge
//! dependence structure built ONCE per function content version from its
//! [`FunctionBodySkeleton`](super::FunctionBodySkeleton) — and from the
//! skeleton ALONE. [`build_function_flow_graph`] takes only
//! `&FunctionBodySkeleton`, so a graph build can never re-walk the AST,
//! observe a query demand, lower a type, or produce a fact.
//!
//! Nodes are the function's bindings (value-definition hubs), expression
//! sites, return sites, and control regions. Edges are TYPED — one class
//! per dependence kind — and split into two families with different
//! reachability stop conditions:
//!
//! - **Value-provider edges** ([`FlowEdgeKind::ValueDef`] +
//!   [`FlowEdgeKind::PathWrite`]) compute which sources provide a demanded
//!   value; a planner MAY stop following them at a definite-present write
//!   for the demanded path head.
//! - **Effect edges** ([`FlowEdgeKind::EvalEffect`] +
//!   [`FlowEdgeKind::ControlRegion`]) stay live past a definite-present
//!   write: a value-dead sibling (an overwritten duplicate key, a spread
//!   source, a computed key) keeps its evaluation-effect edges, because
//!   evaluation effects survive a definite write even though value
//!   materialization does not.
//!
//! The edge vocabulary is open by construction: further dependence classes
//! (narrowing predicates, closure escapes, loop summaries, `try`/`finally`
//! overrides) extend [`FlowEdgeKind`] on this SAME graph — a second flow
//! structure is forbidden.

use std::sync::Arc;

use rustc_hash::FxHashSet;
use verter_no_typeexpr::NoTypeExpr;

use super::{
    FunctionBodySkeleton, SkeletonBindingId, SkeletonCallee, SkeletonExprShape, SkeletonExprSiteId,
    SkeletonObjectEntry, SkeletonObjectKey, SkeletonPathSegment, SkeletonRegionId,
    SkeletonReturnSiteId, SkeletonWriteCertainty, SkeletonWriteTarget,
};

#[cfg(test)]
#[path = "flow_graph_tests.rs"]
mod flow_graph_tests;

/// The executable-region kind a flow graph covers. The graph is ONE region
/// kind today; other executable region kinds (module top-level, static
/// blocks, field / parameter initializers, decorator expressions) enter
/// through this discriminant without reshaping the demand planner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
pub enum ExecutableRegionKind {
    /// An authored function body.
    Function,
}

/// One node of the flow graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
pub struct FlowNodeId(u32);

impl FlowNodeId {
    /// The dense node index.
    #[must_use]
    pub fn index(self) -> usize {
        self.0 as usize
    }

    #[cfg(test)]
    pub(crate) fn from_index(index: u32) -> Self {
        Self(index)
    }
}

/// What one flow-graph node stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FlowNodeKind {
    /// A lexical binding — the value-definition hub of one slot: its
    /// out-edges enumerate the slot's definitions in source order.
    Binding(SkeletonBindingId),
    /// A tracked expression site.
    ExprSite(SkeletonExprSiteId),
    /// A `return` site.
    ReturnSite(SkeletonReturnSiteId),
    /// A control region.
    Region(SkeletonRegionId),
}

/// The typed dependence class of one edge.
#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr)]
pub enum FlowEdgeKind {
    /// The source node's value is provided by the target: a return site's
    /// argument, a binding's initializer / whole-slot definite write
    /// (reaching definition), or an expression site's read of a binding.
    ValueDef,
    /// A write targets a projection path on the source node's value: an
    /// object-literal entry provisioning a key, a member write on a slot,
    /// an optional / unknown write (spread, computed key, logical
    /// assignment).
    PathWrite {
        /// The written projection path (empty = whole-slot, non-definite).
        path: Arc<[SkeletonPathSegment]>,
        /// Whether the write definitely happens when its site evaluates.
        certainty: SkeletonWriteCertainty,
    },
    /// Evaluating the source affects the target: a site's contained write
    /// / call into a binding, or a container's evaluation of an effectful
    /// child site. Stays live past definite-present value writes.
    EvalEffect,
    /// The source node belongs to (or nests inside) the target region.
    ControlRegion,
}

/// The edge-class family discriminant (the reachability stop-condition
/// families).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
pub enum FlowEdgeClass {
    /// Value-provider: may stop at a definite-present write.
    ValueDef,
    /// Value-provider: path-targeted writes.
    PathWrite,
    /// Effect: stays live past value writes.
    EvalEffect,
    /// Effect: control-region membership / nesting.
    ControlRegion,
}

impl FlowEdgeKind {
    /// The edge's class discriminant.
    #[must_use]
    pub fn class(&self) -> FlowEdgeClass {
        match self {
            FlowEdgeKind::ValueDef => FlowEdgeClass::ValueDef,
            FlowEdgeKind::PathWrite { .. } => FlowEdgeClass::PathWrite,
            FlowEdgeKind::EvalEffect => FlowEdgeClass::EvalEffect,
            FlowEdgeKind::ControlRegion => FlowEdgeClass::ControlRegion,
        }
    }
}

/// One typed edge.
#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr)]
pub struct FlowEdge {
    /// The depending node.
    pub from: FlowNodeId,
    /// The provider / affected node.
    pub to: FlowNodeId,
    /// The typed dependence class.
    pub kind: FlowEdgeKind,
    /// Source-order ordinal among `from`'s out-edges of the same class.
    pub ordinal: u32,
}

/// The sparse per-function dependence graph. Arena-free
/// (`Send + Sync + 'static`), compact interned ids throughout, no stored
/// lowered type (`NoTypeExpr`-certified); every type along an edge resolves
/// on demand only when a demand slice traverses it.
#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr)]
pub struct FunctionFlowGraph {
    /// The executable-region kind this graph covers.
    pub region_kind: ExecutableRegionKind,
    binding_count: u32,
    expr_site_count: u32,
    return_site_count: u32,
    region_count: u32,
    /// Every edge, grouped by `from` node (CSR layout).
    edges: Arc<[FlowEdge]>,
    /// CSR offsets: node `n`'s out-edges are `edges[offsets[n]..offsets[n+1]]`.
    offsets: Arc<[u32]>,
}

impl FunctionFlowGraph {
    /// Total node count.
    #[must_use]
    pub fn node_count(&self) -> usize {
        (self.binding_count + self.expr_site_count + self.return_site_count + self.region_count)
            as usize
    }

    /// Every edge, grouped by `from` node.
    #[must_use]
    pub fn edges(&self) -> &[FlowEdge] {
        &self.edges
    }

    /// The node of one binding.
    #[must_use]
    pub fn binding_node(&self, id: SkeletonBindingId) -> FlowNodeId {
        debug_assert!((id.index() as u32) < self.binding_count);
        FlowNodeId(id.index() as u32)
    }

    /// The node of one expression site.
    #[must_use]
    pub fn expr_site_node(&self, id: SkeletonExprSiteId) -> FlowNodeId {
        debug_assert!((id.index() as u32) < self.expr_site_count);
        FlowNodeId(self.binding_count + id.index() as u32)
    }

    /// The node of one return site.
    #[must_use]
    pub fn return_site_node(&self, id: SkeletonReturnSiteId) -> FlowNodeId {
        debug_assert!((id.index() as u32) < self.return_site_count);
        FlowNodeId(self.binding_count + self.expr_site_count + id.index() as u32)
    }

    /// The node of one control region.
    #[must_use]
    pub fn region_node(&self, id: SkeletonRegionId) -> FlowNodeId {
        debug_assert!((id.index() as u32) < self.region_count);
        FlowNodeId(
            self.binding_count + self.expr_site_count + self.return_site_count + id.index() as u32,
        )
    }

    /// What `node` stands for.
    #[must_use]
    pub fn node_kind(&self, node: FlowNodeId) -> FlowNodeKind {
        let index = node.0;
        if index < self.binding_count {
            return FlowNodeKind::Binding(SkeletonBindingId::from_index(index));
        }
        let index = index - self.binding_count;
        if index < self.expr_site_count {
            return FlowNodeKind::ExprSite(SkeletonExprSiteId::from_index(index));
        }
        let index = index - self.expr_site_count;
        if index < self.return_site_count {
            return FlowNodeKind::ReturnSite(SkeletonReturnSiteId::from_index(index));
        }
        FlowNodeKind::Region(SkeletonRegionId::from_index(index - self.return_site_count))
    }

    /// The out-edges of `node`, source-ordered within each class.
    #[must_use]
    pub fn out_edges(&self, node: FlowNodeId) -> &[FlowEdge] {
        let start = self.offsets[node.index()] as usize;
        let end = self.offsets[node.index() + 1] as usize;
        &self.edges[start..end]
    }
}

/// Build the [`FunctionFlowGraph`] of one skeleton. Pure and deterministic
/// over the skeleton alone: no AST, no demand, no type lowering, no
/// resolution dispatch, no route lookup, no fact production.
#[must_use]
pub fn build_function_flow_graph(skeleton: &FunctionBodySkeleton) -> FunctionFlowGraph {
    let binding_count = u32::try_from(skeleton.bindings.len()).unwrap_or(u32::MAX);
    let expr_site_count = u32::try_from(skeleton.expr_sites.len()).unwrap_or(u32::MAX);
    let return_site_count = u32::try_from(skeleton.return_sites.len()).unwrap_or(u32::MAX);
    let region_count = u32::try_from(skeleton.regions.len()).unwrap_or(u32::MAX);

    let binding_node = |id: SkeletonBindingId| FlowNodeId(id.index() as u32);
    let site_node = |id: SkeletonExprSiteId| FlowNodeId(binding_count + id.index() as u32);
    let return_node =
        |id: SkeletonReturnSiteId| FlowNodeId(binding_count + expr_site_count + id.index() as u32);
    let region_node = |id: SkeletonRegionId| {
        FlowNodeId(binding_count + expr_site_count + return_site_count + id.index() as u32)
    };

    // Effectful closure per site: own write / call footprint, or an
    // effectful child (children always follow their parents in the table).
    let mut effectful = vec![false; skeleton.expr_sites.len()];
    for write in skeleton.writes.iter() {
        effectful[write.site.index()] = true;
    }
    for (index, site) in skeleton.expr_sites.iter().enumerate() {
        if !site.calls.is_empty() {
            effectful[index] = true;
        }
    }
    for index in (0..skeleton.expr_sites.len()).rev() {
        if effectful[index] {
            if let Some(parent) = skeleton.expr_sites[index].parent {
                effectful[parent.index()] = true;
            }
        }
    }

    // Lexical binding resolution: a read / callee root / write target of
    // `name` evaluated in `region` binds to the declaration(s) of the
    // NEAREST enclosing region carrying that name — an innermost-first
    // walk of the region parent chain the skeleton already records
    // (`SkeletonBinding.region`, `SkeletonRegion.parent`). A shadowed
    // same-named OUTER binding therefore gets NO dependence edge, so an
    // irrelevant outer initializer can never enter a demand slice through
    // name-keyed fan-out. Only when the enclosing chain carries NO
    // declaration does the resolution fall back to EVERY same-name
    // binding — the conservative arm for genuinely ambiguous hoisting
    // (`var` / nested function declarations living in non-enclosing
    // regions still hoist to function scope).
    let bindings_of_name_in_scope = |name: super::FlowNameId, region: SkeletonRegionId| {
        let mut current = Some(region);
        while let Some(enclosing) = current {
            let mut hits: Vec<SkeletonBindingId> = Vec::new();
            for (index, binding) in skeleton.bindings.iter().enumerate() {
                if binding.name == name && binding.region == enclosing {
                    hits.push(SkeletonBindingId::from_index(index as u32));
                }
            }
            if !hits.is_empty() {
                return hits;
            }
            current = skeleton.regions[enclosing.index()].parent;
        }
        skeleton
            .bindings
            .iter()
            .enumerate()
            .filter(|(_, binding)| binding.name == name)
            .map(|(index, _)| SkeletonBindingId::from_index(index as u32))
            .collect()
    };

    let mut edges: Vec<(FlowNodeId, FlowNodeId, FlowEdgeKind)> = Vec::new();
    // Read / call-effect edges deduplicate per (from, to, class); write and
    // shape edges are never deduplicated — distinct definitions and
    // distinct entries are distinct dependence facts.
    let mut seen: FxHashSet<(u32, u32, FlowEdgeClass)> = FxHashSet::default();

    // Region nesting.
    for (index, region) in skeleton.regions.iter().enumerate() {
        if let Some(parent) = region.parent {
            edges.push((
                region_node(SkeletonRegionId::from_index(index as u32)),
                region_node(parent),
                FlowEdgeKind::ControlRegion,
            ));
        }
    }

    // Bindings: region membership + initializer definition.
    for (index, binding) in skeleton.bindings.iter().enumerate() {
        let node = binding_node(SkeletonBindingId::from_index(index as u32));
        edges.push((
            node,
            region_node(binding.region),
            FlowEdgeKind::ControlRegion,
        ));
        if let Some(initializer) = binding.initializer {
            edges.push((node, site_node(initializer), FlowEdgeKind::ValueDef));
        }
    }

    // Expression sites: region membership, reads, call effects, container
    // effects, object-shape path writes.
    for (index, site) in skeleton.expr_sites.iter().enumerate() {
        let id = SkeletonExprSiteId::from_index(index as u32);
        let node = site_node(id);
        edges.push((node, region_node(site.region), FlowEdgeKind::ControlRegion));
        for read in site.reads.iter() {
            for binding in bindings_of_name_in_scope(read.name, site.region) {
                let to = binding_node(binding);
                if seen.insert((node.0, to.0, FlowEdgeClass::ValueDef)) {
                    edges.push((node, to, FlowEdgeKind::ValueDef));
                }
            }
        }
        for call in site.calls.iter() {
            let root = match &call.callee {
                SkeletonCallee::Named(name) => Some(*name),
                SkeletonCallee::Path(path) => path.first().copied(),
                SkeletonCallee::Opaque => None,
            };
            if let Some(root) = root {
                for binding in bindings_of_name_in_scope(root, site.region) {
                    let to = binding_node(binding);
                    if seen.insert((node.0, to.0, FlowEdgeClass::EvalEffect)) {
                        edges.push((node, to, FlowEdgeKind::EvalEffect));
                    }
                }
            }
        }
        if effectful[index] {
            if let Some(parent) = site.parent {
                edges.push((site_node(parent), node, FlowEdgeKind::EvalEffect));
            }
        }
        if let SkeletonExprShape::ObjectLiteral { entries } = &site.shape {
            for entry in entries.iter() {
                match entry {
                    SkeletonObjectEntry::Property { key, value, .. } => {
                        let path: Arc<[SkeletonPathSegment]> = match key {
                            SkeletonObjectKey::Static(name) => Arc::from(
                                vec![SkeletonPathSegment::Static(*name)].into_boxed_slice(),
                            ),
                            SkeletonObjectKey::Computed(_) => {
                                Arc::from(vec![SkeletonPathSegment::Computed].into_boxed_slice())
                            }
                        };
                        edges.push((
                            node,
                            site_node(*value),
                            FlowEdgeKind::PathWrite {
                                path,
                                certainty: SkeletonWriteCertainty::Definite,
                            },
                        ));
                    }
                    SkeletonObjectEntry::Spread { source } => {
                        edges.push((
                            node,
                            site_node(*source),
                            FlowEdgeKind::PathWrite {
                                path: Arc::from(
                                    vec![SkeletonPathSegment::Computed].into_boxed_slice(),
                                ),
                                certainty: SkeletonWriteCertainty::Optional,
                            },
                        ));
                    }
                }
            }
        }
    }

    // Writes: slot definitions + evaluation effects, in source order.
    for write in skeleton.writes.iter() {
        let SkeletonWriteTarget::Named(name) = write.target else {
            continue;
        };
        let provider = site_node(write.value.unwrap_or(write.site));
        for binding in bindings_of_name_in_scope(name, write.region) {
            let hub = binding_node(binding);
            if write.path.is_empty() && matches!(write.certainty, SkeletonWriteCertainty::Definite)
            {
                edges.push((hub, provider, FlowEdgeKind::ValueDef));
            } else {
                edges.push((
                    hub,
                    provider,
                    FlowEdgeKind::PathWrite {
                        path: Arc::clone(&write.path),
                        certainty: write.certainty,
                    },
                ));
            }
            let effect_from = site_node(write.site);
            if seen.insert((
                effect_from.0,
                binding_node(binding).0,
                FlowEdgeClass::EvalEffect,
            )) {
                edges.push((effect_from, binding_node(binding), FlowEdgeKind::EvalEffect));
            }
        }
    }

    // Return sites: region membership, argument value, argument effects.
    for (index, return_site) in skeleton.return_sites.iter().enumerate() {
        let node = return_node(SkeletonReturnSiteId::from_index(index as u32));
        edges.push((
            node,
            region_node(return_site.region),
            FlowEdgeKind::ControlRegion,
        ));
        if let Some(argument) = return_site.argument {
            edges.push((node, site_node(argument), FlowEdgeKind::ValueDef));
            if effectful[argument.index()] {
                edges.push((node, site_node(argument), FlowEdgeKind::EvalEffect));
            }
        }
    }

    // CSR finalize: stable-sort by `from` (emission order is source order
    // within a node), assign per-(from, class) ordinals, build offsets.
    let node_count = (binding_count + expr_site_count + return_site_count + region_count) as usize;
    let mut order: Vec<usize> = (0..edges.len()).collect();
    order.sort_by_key(|&index| edges[index].0 .0);
    let mut finalized: Vec<FlowEdge> = Vec::with_capacity(edges.len());
    let mut offsets: Vec<u32> = vec![0; node_count + 1];
    {
        let mut counts: Vec<u32> = vec![0; node_count];
        for &index in &order {
            counts[edges[index].0.index()] += 1;
        }
        let mut running = 0u32;
        for (node, count) in counts.iter().enumerate() {
            offsets[node] = running;
            running += count;
            offsets[node + 1] = running;
        }
    }
    let mut class_counters: rustc_hash::FxHashMap<(u32, FlowEdgeClass), u32> =
        rustc_hash::FxHashMap::default();
    for &index in &order {
        let (from, to, kind) = edges[index].clone();
        let counter = class_counters.entry((from.0, kind.class())).or_insert(0);
        let ordinal = *counter;
        *counter += 1;
        finalized.push(FlowEdge {
            from,
            to,
            kind,
            ordinal,
        });
    }

    FunctionFlowGraph {
        region_kind: ExecutableRegionKind::Function,
        binding_count,
        expr_site_count,
        return_site_count,
        region_count,
        edges: Arc::from(finalized.into_boxed_slice()),
        offsets: Arc::from(offsets.into_boxed_slice()),
    }
}
