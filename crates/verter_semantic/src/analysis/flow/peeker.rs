//! [`ReturnPathPeeker`] — the graph demand PLANNER over one
//! [`FunctionFlowGraph`].
//!
//! The planner computes a demand slice as **graph reachability** from a
//! demand origin (a return site or an expression site) across the typed
//! edge classes, under the two-frontier rule expressed AS edge classes:
//!
//! - **Value-provider edges** ([`FlowEdgeClass::ValueDef`] +
//!   [`FlowEdgeClass::PathWrite`]) compute which sources provide the
//!   demanded value. Path-write scans run right-to-left (descending
//!   source ordinal) and MAY stop at a definite-present write for the
//!   demanded path head; optional / unknown writes stay reachable and
//!   earlier candidates remain reachable past them.
//! - **Effect edges** ([`FlowEdgeClass::EvalEffect`] +
//!   [`FlowEdgeClass::ControlRegion`]) stay live past a definite-present
//!   write: a value-dead sibling keeps its evaluation-effect reachability
//!   because evaluation effects survive a definite write even though
//!   value materialization does not.
//!
//! The planner holds ONLY the graph — its input type makes a procedural
//! statement / AST / skeleton walk impossible by construction: no
//! statement list, no OXC node, and no skeleton is reachable from
//! [`ReturnPathPeeker`]. It selects a reachable subgraph; it never
//! re-discovers structure the graph build already captured. The one
//! skeleton-adjacent operation — resolving demanded property-key TEXT to
//! interned [`FlowNameId`]s — happens in the [`SliceDemand`] constructor
//! BEFORE planning, as a name-table lookup, never a body walk.

use std::sync::Arc;

use rustc_hash::FxHashSet;
use verter_no_typeexpr::NoTypeExpr;

use super::flow_graph::{FlowEdge, FlowEdgeClass, FlowEdgeKind, FlowNodeId, FunctionFlowGraph};
use super::flow_ir::ReturnSlicePlan;
use super::{
    FlowNameId, FunctionBodySkeleton, SkeletonExprSiteId, SkeletonPathSegment,
    SkeletonReturnSiteId, SkeletonWriteCertainty,
};

#[cfg(test)]
#[path = "peeker_tests.rs"]
mod peeker_tests;

// ---------------------------------------------------------------------------
// Demand
// ---------------------------------------------------------------------------

/// One demand origin: a return site or an arbitrary expression site of
/// the sliced function. The same planner serves return-type demands and
/// expression-site demands — no second flow engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
pub enum SliceOrigin {
    /// A `return` site of the sliced function.
    Return(SkeletonReturnSiteId),
    /// An arbitrary tracked expression site.
    Expr(SkeletonExprSiteId),
}

/// One segment of the demanded projection path, resolved against the
/// skeleton's interned name table.
#[derive(Debug, Clone, PartialEq, Eq, Hash, NoTypeExpr)]
pub enum DemandSegment {
    /// A demanded property key that is interned in the sliced body — it
    /// can match static path-write segments.
    Named(FlowNameId),
    /// A demanded property key never mentioned in the sliced body: no
    /// static path-write can match it (only computed / unknown writes
    /// stay candidate providers). The authored key text is retained for
    /// slice identity.
    Foreign(Arc<str>),
}

/// One demand triple `(origins, projection path)` over a function flow
/// graph. Origins are a SET so a whole-return demand (every return site)
/// plans as one multi-source reachability — the union is the traversal,
/// not a second composition pass.
#[derive(Debug, Clone, PartialEq, Eq, NoTypeExpr)]
pub struct SliceDemand {
    /// The demand origins.
    pub origins: Arc<[SliceOrigin]>,
    /// The demanded projection path under the origin's value (empty =
    /// the whole value).
    pub path: Arc<[DemandSegment]>,
}

impl SliceDemand {
    /// The whole-return-surface demand for `path` under every return
    /// site of `skeleton`: origins = every return site, and each
    /// demanded key resolved against the skeleton's interned name table
    /// (a table lookup — not a body walk; a key the body never mentions
    /// stays [`DemandSegment::Foreign`]).
    #[must_use]
    pub fn for_return_projection(skeleton: &FunctionBodySkeleton, path: &[Arc<str>]) -> Self {
        let origins: Vec<SliceOrigin> = (0..skeleton.return_sites.len())
            .filter_map(|index| u32::try_from(index).ok())
            .map(|index| SliceOrigin::Return(SkeletonReturnSiteId::from_index(index)))
            .collect();
        let segments: Vec<DemandSegment> = path
            .iter()
            .map(|name| match skeleton.name_id(name) {
                Some(id) => DemandSegment::Named(id),
                None => DemandSegment::Foreign(Arc::clone(name)),
            })
            .collect();
        Self {
            origins: Arc::from(origins.into_boxed_slice()),
            path: Arc::from(segments.into_boxed_slice()),
        }
    }

    /// A single-origin expression-site demand for `path`.
    #[must_use]
    pub fn for_expression_site(
        skeleton: &FunctionBodySkeleton,
        site: SkeletonExprSiteId,
        path: &[Arc<str>],
    ) -> Self {
        let segments: Vec<DemandSegment> = path
            .iter()
            .map(|name| match skeleton.name_id(name) {
                Some(id) => DemandSegment::Named(id),
                None => DemandSegment::Foreign(Arc::clone(name)),
            })
            .collect();
        Self {
            origins: Arc::from(vec![SliceOrigin::Expr(site)].into_boxed_slice()),
            path: Arc::from(segments.into_boxed_slice()),
        }
    }
}

// ---------------------------------------------------------------------------
// Budget
// ---------------------------------------------------------------------------

/// The demand-slice budget. Armed by default: [`Default`] carries the
/// production caps, and every trip returns a typed
/// [`FlowSliceBudgetExceeded`] — never a panic, never a silent partial
/// plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
pub struct FlowSliceBudget {
    /// Maximum demand-origin return sites.
    pub max_return_sites: u32,
    /// Maximum selected nodes (value + effect + region) in one slice.
    pub max_selected_nodes: u32,
}

impl Default for FlowSliceBudget {
    fn default() -> Self {
        Self {
            max_return_sites: 256,
            max_selected_nodes: 4096,
        }
    }
}

/// Which budget axis tripped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
pub enum FlowSliceBudgetAxis {
    /// Too many demand-origin return sites.
    ReturnSites,
    /// Too many selected nodes.
    SelectedNodes,
}

/// A typed budget trip: the axis, its limit, and the observed count at
/// the trip. A genuine partial — the caller must route it through
/// non-admission (`ReturnOnly` semantics); it never becomes a plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, NoTypeExpr)]
pub struct FlowSliceBudgetExceeded {
    /// The tripped axis.
    pub axis: FlowSliceBudgetAxis,
    /// The configured limit of the tripped axis.
    pub limit: u32,
    /// The observed count when the limit tripped.
    pub observed: u32,
}

// ---------------------------------------------------------------------------
// The planner
// ---------------------------------------------------------------------------

/// The graph demand planner. Holds ONLY the graph — see the module doc
/// for why that is the structural proof the slice is reachability, not a
/// procedural walk.
pub struct ReturnPathPeeker<'g> {
    graph: &'g FunctionFlowGraph,
}

/// One worklist item of the two-frontier reachability.
enum WorkItem {
    /// Value-provider frontier: the node's value contributes to the
    /// demanded projection `path[path_start..]`.
    Value {
        /// The reached node.
        node: FlowNodeId,
        /// Start index of the remaining demanded path.
        path_start: u32,
    },
    /// Effect frontier: the node's evaluation affects the slice even
    /// when its value is non-contributing.
    Effect {
        /// The reached node.
        node: FlowNodeId,
    },
}

impl<'g> ReturnPathPeeker<'g> {
    /// A planner over `graph`.
    #[must_use]
    pub fn new(graph: &'g FunctionFlowGraph) -> Self {
        Self { graph }
    }

    /// Compute the demand slice for `demand` as graph reachability from
    /// its origins, bounded by `budget`. The result is exactly the
    /// reachable subgraph under the two edge-class families' stop
    /// conditions; an over-budget traversal returns the typed
    /// [`FlowSliceBudgetExceeded`].
    pub fn plan(
        &self,
        demand: &SliceDemand,
        budget: &FlowSliceBudget,
    ) -> Result<ReturnSlicePlan, FlowSliceBudgetExceeded> {
        let return_origins = demand
            .origins
            .iter()
            .filter(|origin| matches!(origin, SliceOrigin::Return(_)))
            .count();
        if return_origins > budget.max_return_sites as usize {
            return Err(FlowSliceBudgetExceeded {
                axis: FlowSliceBudgetAxis::ReturnSites,
                limit: budget.max_return_sites,
                observed: u32::try_from(return_origins).unwrap_or(u32::MAX),
            });
        }

        let mut state = PlanState {
            demand_path: &demand.path,
            value_nodes: FxHashSet::default(),
            effect_nodes: FxHashSet::default(),
            selected: FxHashSet::default(),
            value_visited: FxHashSet::default(),
            effect_visited: FxHashSet::default(),
            worklist: Vec::new(),
        };

        for origin in demand.origins.iter() {
            let node = match origin {
                SliceOrigin::Return(id) => self.graph.return_site_node(*id),
                SliceOrigin::Expr(id) => self.graph.expr_site_node(*id),
            };
            state.worklist.push(WorkItem::Value {
                node,
                path_start: 0,
            });
        }

        while let Some(item) = state.worklist.pop() {
            match item {
                WorkItem::Value { node, path_start } => {
                    self.process_value(&mut state, node, path_start, budget)?;
                }
                WorkItem::Effect { node } => {
                    self.process_effect(&mut state, node, budget)?;
                }
            }
        }

        let mut value: Vec<FlowNodeId> = state.value_nodes.into_iter().collect();
        value.sort_by_key(|node| node.index());
        let mut effect_only: Vec<FlowNodeId> = state
            .effect_nodes
            .into_iter()
            .filter(|node| !value.iter().any(|selected| selected == node))
            .collect();
        effect_only.sort_by_key(|node| node.index());

        Ok(ReturnSlicePlan {
            origins: Arc::clone(&demand.origins),
            demand_path: Arc::clone(&demand.path),
            value_nodes: Arc::from(value.into_boxed_slice()),
            effect_only_nodes: Arc::from(effect_only.into_boxed_slice()),
        })
    }

    /// Value-frontier step: select `node` for value, then follow its
    /// value-provider out-edges under the demanded-path rules. Every
    /// value selection also enters the effect frontier (evaluating a
    /// provider runs its effects).
    fn process_value(
        &self,
        state: &mut PlanState<'_>,
        node: FlowNodeId,
        path_start: u32,
        budget: &FlowSliceBudget,
    ) -> Result<(), FlowSliceBudgetExceeded> {
        if !state.value_visited.insert((node, path_start)) {
            return Ok(());
        }
        state.value_nodes.insert(node);
        select(state, node, budget)?;
        state.worklist.push(WorkItem::Effect { node });

        let remaining_len = state.demand_path.len().saturating_sub(path_start as usize);
        let edges = self.graph.out_edges(node);

        // Value-def edges thread the remaining demand unchanged: return
        // argument, reaching definition, binding read.
        for edge in edges {
            if edge.kind.class() == FlowEdgeClass::ValueDef {
                state.worklist.push(WorkItem::Value {
                    node: edge.to,
                    path_start,
                });
            }
        }

        if remaining_len == 0 {
            // Whole-value demand: every path-write entry contributes its
            // whole written value.
            for edge in edges {
                if edge.kind.class() == FlowEdgeClass::PathWrite {
                    state.worklist.push(WorkItem::Value {
                        node: edge.to,
                        path_start,
                    });
                }
            }
            return Ok(());
        }

        // Path-write scan, right-to-left (descending source ordinal): a
        // definite-present static write for the demanded head stops the
        // scan for VALUE; optional / unknown writes stay reachable and
        // earlier candidates remain reachable past them. Effect
        // reachability is untouched — it flows through the effect
        // frontier regardless of this stop.
        let path_writes: Vec<&FlowEdge> = edges
            .iter()
            .filter(|edge| edge.kind.class() == FlowEdgeClass::PathWrite)
            .collect();
        for edge in path_writes.into_iter().rev() {
            let FlowEdgeKind::PathWrite { path, certainty } = &edge.kind else {
                continue;
            };
            match match_write_path(path, state.demand_path, path_start) {
                WritePathMatch::None => {}
                WritePathMatch::Static { consumed } => {
                    state.worklist.push(WorkItem::Value {
                        node: edge.to,
                        path_start: path_start + consumed,
                    });
                    if *certainty == SkeletonWriteCertainty::Definite
                        && path.len() == 1
                        && consumed == 1
                    {
                        // Definite-present write for the demanded head:
                        // the value is fully determined here — earlier
                        // candidates are value-suppressed.
                        break;
                    }
                }
                WritePathMatch::Unknown { consumed } => {
                    // An unknown-key write may either provision the
                    // demanded key (consume it) or merge a whole source
                    // object (spread — the demand projects INTO the
                    // source unshifted). Both interpretations stay
                    // reachable; neither stops the scan.
                    state.worklist.push(WorkItem::Value {
                        node: edge.to,
                        path_start: path_start + consumed,
                    });
                    state.worklist.push(WorkItem::Value {
                        node: edge.to,
                        path_start,
                    });
                }
            }
        }
        Ok(())
    }

    /// Effect-frontier step: select `node` for effect and follow ONLY
    /// the effect-family out-edges (eval-effect + control-region). Value
    /// materialization never enters through this frontier.
    fn process_effect(
        &self,
        state: &mut PlanState<'_>,
        node: FlowNodeId,
        budget: &FlowSliceBudget,
    ) -> Result<(), FlowSliceBudgetExceeded> {
        if !state.effect_visited.insert(node) {
            return Ok(());
        }
        state.effect_nodes.insert(node);
        select(state, node, budget)?;
        for edge in self.graph.out_edges(node) {
            match edge.kind.class() {
                FlowEdgeClass::EvalEffect | FlowEdgeClass::ControlRegion => {
                    state.worklist.push(WorkItem::Effect { node: edge.to });
                }
                FlowEdgeClass::ValueDef | FlowEdgeClass::PathWrite => {}
            }
        }
        Ok(())
    }
}

/// Traversal state of one plan.
struct PlanState<'d> {
    demand_path: &'d [DemandSegment],
    value_nodes: FxHashSet<FlowNodeId>,
    effect_nodes: FxHashSet<FlowNodeId>,
    selected: FxHashSet<FlowNodeId>,
    value_visited: FxHashSet<(FlowNodeId, u32)>,
    effect_visited: FxHashSet<FlowNodeId>,
    worklist: Vec<WorkItem>,
}

/// Count a node toward the selection budget.
fn select(
    state: &mut PlanState<'_>,
    node: FlowNodeId,
    budget: &FlowSliceBudget,
) -> Result<(), FlowSliceBudgetExceeded> {
    state.selected.insert(node);
    let observed = state.selected.len();
    if observed > budget.max_selected_nodes as usize {
        return Err(FlowSliceBudgetExceeded {
            axis: FlowSliceBudgetAxis::SelectedNodes,
            limit: budget.max_selected_nodes,
            observed: u32::try_from(observed).unwrap_or(u32::MAX),
        });
    }
    Ok(())
}

/// How a write's projection path relates to the remaining demand.
enum WritePathMatch {
    /// A known-unrelated write: no value-provider reachability.
    None,
    /// A static match: the write provisions the demanded head; `consumed`
    /// demand segments are satisfied by the write's path.
    Static {
        /// Demand segments consumed by the write path.
        consumed: u32,
    },
    /// An unknown-key (computed / spread) write: `consumed` segments are
    /// satisfied under the provisioning interpretation; the merge
    /// interpretation consumes none.
    Unknown {
        /// Demand segments consumed under the provisioning reading.
        consumed: u32,
    },
}

/// Match a write path against the remaining demand `demand[path_start..]`.
/// Static segments must equal the demanded key; computed segments match
/// any demanded key; a write deeper than the demand still provides (its
/// value occupies a sub-path of the demanded value).
fn match_write_path(
    write_path: &[SkeletonPathSegment],
    demand: &[DemandSegment],
    path_start: u32,
) -> WritePathMatch {
    let remaining = &demand[(path_start as usize).min(demand.len())..];
    if write_path.is_empty() {
        // Whole-slot write: provides the whole remaining demand.
        return WritePathMatch::Static { consumed: 0 };
    }
    let mut unknown = false;
    let compared = write_path.len().min(remaining.len());
    for (write_segment, demand_segment) in write_path.iter().zip(remaining.iter()) {
        match (write_segment, demand_segment) {
            (SkeletonPathSegment::Static(write_name), DemandSegment::Named(demand_name)) => {
                if write_name != demand_name {
                    return WritePathMatch::None;
                }
            }
            (SkeletonPathSegment::Static(_), DemandSegment::Foreign(_)) => {
                return WritePathMatch::None;
            }
            (SkeletonPathSegment::Computed, _) => {
                unknown = true;
            }
        }
    }
    let consumed = u32::try_from(compared).unwrap_or(u32::MAX);
    if unknown {
        WritePathMatch::Unknown { consumed }
    } else {
        WritePathMatch::Static { consumed }
    }
}
