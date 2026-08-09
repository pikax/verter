//! [`lower_slice_plan`] — lower EXACTLY one [`ReturnSlicePlan`] into
//! [`FlowSliceIR`].
//!
//! The lowerer rehydrates only the plan's selected nodes from the
//! skeleton records: selected bindings become slots, selected expression
//! sites become expression records (object literals keep only their
//! selected entries, with the elided sibling count recorded), selected
//! writes / calls become source-ordered effect obligations, and the
//! demand's return origins become the return accumulator. Nothing
//! outside the plan is lowered, and no type is lowered at all —
//! expression content stays behind span locators, evaluated on demand at
//! slice-evaluation time.
//!
//! This module never computes a slice hash: the hash-then-lower split is
//! held by the opaque [`FlowSliceHash`](super::hashing::FlowSliceHash)
//! (the lowered-body cache key cannot exist without a hash the hashing
//! producer minted first), and [`FlowSliceIR`] carries no hash field to
//! smuggle one through.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use super::flow_graph::{FlowEdgeKind, FlowNodeId, FlowNodeKind, FunctionFlowGraph};
use super::flow_ir::{
    FlowCallee, FlowDef, FlowEffect, FlowEffectTarget, FlowExpr, FlowExprId, FlowExprRole,
    FlowExprShape, FlowObjectEntry, FlowObjectKey, FlowPath, FlowPathSegment, FlowRead,
    FlowReturnEntry, FlowSliceIR, FlowSlot, FlowSlotId, ReturnAccumulator, ReturnSlicePlan,
};
use super::peeker::{DemandSegment, SliceOrigin};
use super::{
    FunctionBodySkeleton, SkeletonBindingId, SkeletonCallee, SkeletonExprShape, SkeletonExprSiteId,
    SkeletonObjectEntry, SkeletonObjectKey, SkeletonPathSegment, SkeletonWriteTarget,
};

#[cfg(test)]
#[path = "lower_tests.rs"]
mod lower_tests;

/// Lower the plan's selected subgraph into [`FlowSliceIR`]. Pure over
/// `(plan, graph, skeleton)`; touches only selected nodes.
#[must_use]
pub fn lower_slice_plan(
    plan: &ReturnSlicePlan,
    graph: &FunctionFlowGraph,
    skeleton: &FunctionBodySkeleton,
) -> FlowSliceIR {
    // ── Selection tables ────────────────────────────────────────────
    let mut selected_sites: Vec<(SkeletonExprSiteId, FlowExprRole)> = Vec::new();
    let mut selected_bindings: Vec<(SkeletonBindingId, bool)> = Vec::new();
    let mut classify = |node: FlowNodeId, value: bool| match graph.node_kind(node) {
        FlowNodeKind::ExprSite(site) => selected_sites.push((
            site,
            if value {
                FlowExprRole::Value
            } else {
                FlowExprRole::EffectOnly
            },
        )),
        FlowNodeKind::Binding(binding) => selected_bindings.push((binding, value)),
        FlowNodeKind::ReturnSite(_) | FlowNodeKind::Region(_) => {}
    };
    for node in plan.value_nodes.iter() {
        classify(*node, true);
    }
    for node in plan.effect_only_nodes.iter() {
        classify(*node, false);
    }
    selected_sites.sort_by_key(|(site, _)| site.index());
    selected_bindings.sort_by_key(|(binding, _)| binding.index());

    let site_ids: FxHashMap<usize, FlowExprId> = selected_sites
        .iter()
        .enumerate()
        .map(|(index, (site, _))| (site.index(), FlowExprId::from_index(index as u32)))
        .collect();
    let slot_ids: FxHashMap<usize, FlowSlotId> = selected_bindings
        .iter()
        .enumerate()
        .map(|(index, (binding, _))| (binding.index(), FlowSlotId::from_index(index as u32)))
        .collect();
    let expr_id = |site: SkeletonExprSiteId| site_ids.get(&site.index()).copied();

    // Name → slot, only when exactly one selected binding carries the
    // name (shadow-ambiguous names resolve lexically at solve time).
    let mut slot_by_name: FxHashMap<Arc<str>, Option<FlowSlotId>> = FxHashMap::default();
    for (binding, _) in &selected_bindings {
        let record = skeleton.binding(*binding);
        let name: Arc<str> = Arc::from(skeleton.name(record.name));
        let slot = slot_ids.get(&binding.index()).copied();
        slot_by_name
            .entry(name)
            .and_modify(|existing| *existing = None)
            .or_insert(slot);
    }
    let slot_of_name = |name: &str| slot_by_name.get(name).copied().flatten();

    // ── Slots ───────────────────────────────────────────────────────
    let slots: Vec<FlowSlot> = selected_bindings
        .iter()
        .map(|(binding, value_selected)| {
            let record = skeleton.binding(*binding);
            let hub = graph.binding_node(*binding);
            let mut defs: Vec<FlowDef> = Vec::new();
            for edge in graph.out_edges(hub) {
                let (path, certainty) = match &edge.kind {
                    FlowEdgeKind::ValueDef => (
                        Arc::from(Vec::new().into_boxed_slice()),
                        super::SkeletonWriteCertainty::Definite,
                    ),
                    FlowEdgeKind::PathWrite { path, certainty } => {
                        (lower_path(path, skeleton), *certainty)
                    }
                    FlowEdgeKind::EvalEffect | FlowEdgeKind::ControlRegion => continue,
                };
                let FlowNodeKind::ExprSite(site) = graph.node_kind(edge.to) else {
                    continue;
                };
                let Some(value) = expr_id(site) else {
                    continue;
                };
                defs.push(FlowDef {
                    value,
                    path,
                    certainty,
                });
            }
            FlowSlot {
                name: Arc::from(skeleton.name(record.name)),
                kind: record.kind,
                binding: *binding,
                span: record.span,
                value_selected: *value_selected,
                defs: Arc::from(defs.into_boxed_slice()),
            }
        })
        .collect();

    // ── Expressions ─────────────────────────────────────────────────
    let exprs: Vec<FlowExpr> = selected_sites
        .iter()
        .map(|(site_id, role)| {
            let site = skeleton.expr_site(*site_id);
            let shape = match &site.shape {
                SkeletonExprShape::ObjectLiteral { entries } => {
                    let mut lowered: Vec<FlowObjectEntry> = Vec::new();
                    let mut elided: u32 = 0;
                    for entry in entries.iter() {
                        match entry {
                            SkeletonObjectEntry::Property { key, value, .. } => {
                                match expr_id(*value) {
                                    Some(value) => lowered.push(FlowObjectEntry::Property {
                                        key: match key {
                                            SkeletonObjectKey::Static(name) => {
                                                FlowObjectKey::Named(Arc::from(
                                                    skeleton.name(*name),
                                                ))
                                            }
                                            SkeletonObjectKey::Computed(key_site) => {
                                                FlowObjectKey::Computed(expr_id(*key_site))
                                            }
                                        },
                                        value,
                                    }),
                                    None => elided += 1,
                                }
                            }
                            SkeletonObjectEntry::Spread { source } => match expr_id(*source) {
                                Some(source) => {
                                    lowered.push(FlowObjectEntry::Spread { source });
                                }
                                None => elided += 1,
                            },
                        }
                    }
                    FlowExprShape::ObjectLiteral {
                        entries: Arc::from(lowered.into_boxed_slice()),
                        elided_entries: elided,
                    }
                }
                // A branch JOIN carries no lowered shape of its own: its
                // arms ride as their OWN selected expression records
                // (the graph made them value providers), and the content
                // half re-reads the authored branch structure from the
                // retained snapshot through this site's span locator —
                // exactly as it does for any other non-object site.
                SkeletonExprShape::BranchJoin { .. } | SkeletonExprShape::Other => {
                    FlowExprShape::Opaque {
                        reads: Arc::from(
                            site.reads
                                .iter()
                                .map(|read| {
                                    let name = skeleton.name(read.name);
                                    FlowRead {
                                        name: Arc::from(name),
                                        slot: slot_of_name(name),
                                    }
                                })
                                .collect::<Vec<_>>()
                                .into_boxed_slice(),
                        ),
                    }
                }
            };
            FlowExpr {
                site: *site_id,
                span: site.span,
                role: *role,
                shape,
            }
        })
        .collect();

    // ── Effects (source order via span start) ───────────────────────
    let mut effects: Vec<FlowEffect> = Vec::new();
    for write in skeleton.writes.iter() {
        let Some(site) = expr_id(write.site) else {
            continue;
        };
        let target = match write.target {
            SkeletonWriteTarget::Named(name) => {
                let text = skeleton.name(name);
                match slot_of_name(text) {
                    Some(slot) => FlowEffectTarget::Slot(slot),
                    None => FlowEffectTarget::Named(Arc::from(text)),
                }
            }
            SkeletonWriteTarget::Opaque => FlowEffectTarget::Opaque,
        };
        effects.push(FlowEffect::Write {
            site,
            target,
            path: lower_path(&write.path, skeleton),
            certainty: write.certainty,
            value: write.value.and_then(expr_id),
            span: write.span,
        });
    }
    for (site_id, _) in &selected_sites {
        let site = skeleton.expr_site(*site_id);
        let Some(id) = expr_id(*site_id) else {
            continue;
        };
        for call in site.calls.iter() {
            effects.push(FlowEffect::Call {
                site: id,
                callee: match &call.callee {
                    SkeletonCallee::Named(name) => {
                        FlowCallee::Named(Arc::from(skeleton.name(*name)))
                    }
                    SkeletonCallee::Path(path) => FlowCallee::Path(Arc::from(
                        path.iter()
                            .map(|segment| Arc::from(skeleton.name(*segment)))
                            .collect::<Vec<Arc<str>>>()
                            .into_boxed_slice(),
                    )),
                    SkeletonCallee::Opaque => FlowCallee::Opaque,
                },
                new_construct: call.new_construct,
                span: call.span,
            });
        }
    }
    // Both families' spans are `FrameSpan`s — the SAME coordinate system —
    // so this key is source order. It could not be anything else: a mixed
    // comparison against an absolute offset does not typecheck.
    effects.sort_by_key(|effect| match effect {
        FlowEffect::Write { span, .. } | FlowEffect::Call { span, .. } => *span,
    });

    // ── Returns + expression origins ────────────────────────────────
    let mut returns: Vec<FlowReturnEntry> = Vec::new();
    let mut expression_origins: Vec<FlowExprId> = Vec::new();
    for origin in plan.origins.iter() {
        match origin {
            SliceOrigin::Return(id) => {
                let site = skeleton.return_site(*id);
                returns.push(FlowReturnEntry {
                    ordinal: site.ordinal,
                    implicit: site.implicit,
                    argument: site.argument.and_then(expr_id),
                    span: site.span,
                });
            }
            SliceOrigin::Expr(id) => {
                if let Some(expr) = expr_id(*id) {
                    expression_origins.push(expr);
                }
            }
        }
    }
    returns.sort_by_key(|entry| entry.ordinal);

    FlowSliceIR {
        demanded_path: lower_demand_path(&plan.demand_path, skeleton),
        slots: Arc::from(slots.into_boxed_slice()),
        exprs: Arc::from(exprs.into_boxed_slice()),
        effects: Arc::from(effects.into_boxed_slice()),
        returns: ReturnAccumulator {
            sites: Arc::from(returns.into_boxed_slice()),
        },
        expression_origins: Arc::from(expression_origins.into_boxed_slice()),
    }
}

fn lower_path(path: &[SkeletonPathSegment], skeleton: &FunctionBodySkeleton) -> FlowPath {
    Arc::from(
        path.iter()
            .map(|segment| match segment {
                SkeletonPathSegment::Static(name) => {
                    FlowPathSegment::Named(Arc::from(skeleton.name(*name)))
                }
                SkeletonPathSegment::Computed => FlowPathSegment::Computed,
            })
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    )
}

fn lower_demand_path(path: &[DemandSegment], skeleton: &FunctionBodySkeleton) -> FlowPath {
    Arc::from(
        path.iter()
            .map(|segment| match segment {
                DemandSegment::Named(name) => {
                    FlowPathSegment::Named(Arc::from(skeleton.name(*name)))
                }
                DemandSegment::Foreign(text) => FlowPathSegment::Named(Arc::clone(text)),
            })
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    )
}
