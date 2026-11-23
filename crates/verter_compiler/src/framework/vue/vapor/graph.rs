//! Graph edges own effect membership, ordering, and distinct read dependencies.
use super::effect::{EffectGroupId, EffectId, VaporEffect};
use super::ops::{RunId, Runs};
use super::repeated_reads::{record, RecordedEffect};
use crate::framework::vue::target::VueTargetRefusal;
use crate::framework_common::structure::{Id, StructureError};
use crate::target_plan::{RelationGraph, RelationGraphBuilder};
use crate::work_ledger::{CompilerWorkLedger, WorkKind};
use rustc_hash::{FxHashMap, FxHashSet};
use verter_audit::observe::ObserveExec;
use verter_span::Span;

pub(super) enum VaporEffectKey {}

#[derive(Debug, Clone, Copy)]
pub(super) enum EffectWrapper {
    Render,
    Once,
    Memo { deps: RunId, cache: u32 },
}

#[derive(Debug, Clone, Copy)]
pub(super) enum VaporEffectNode<'c> {
    Group {
        indent: &'c str,
        wrapper: EffectWrapper,
    },
    Effect {
        row: EffectId,
    },
    Read {
        token: u32,
    },
}

pub(super) type EffectRelations<'c> = RelationGraph<VaporEffectKey, VaporEffectNode<'c>, ()>;

pub(super) struct EffectGraph<'c> {
    builder: Option<RelationGraphBuilder<VaporEffectKey, VaporEffectNode<'c>, ()>>,
    graph: Option<EffectRelations<'c>>,
    pub(super) records: Vec<Option<RecordedEffect<'c>>>,
    pub(super) tokens: Vec<String>,
}
impl Default for EffectGraph<'_> {
    fn default() -> Self {
        Self {
            builder: Some(RelationGraphBuilder::new()),
            graph: None,
            records: Vec::new(),
            tokens: Vec::new(),
        }
    }
}
impl<'c> EffectGraph<'c> {
    pub(super) fn add_group(
        &mut self,
        effects: Vec<EffectId>,
        indent: &'c str,
        wrapper: EffectWrapper,
        table: &[VaporEffect<'c>],
        runs: &Runs<'c>,
        source: &'c str,
    ) -> Result<EffectGroupId, VueTargetRefusal> {
        let builder = self
            .builder
            .as_mut()
            .ok_or_else(|| refusal("sealed-effect-graph"))?;
        let group = builder
            .add_node(VaporEffectNode::Group { indent, wrapper })
            .map_err(storage_refusal)?;
        self.records.resize_with(table.len(), || None);
        let mut tokens: FxHashMap<String, EffectGroupId> = FxHashMap::default();
        for row in effects {
            let effect = builder
                .add_node(VaporEffectNode::Effect { row })
                .map_err(storage_refusal)?;
            builder
                .add_edge(group, effect, ())
                .map_err(storage_refusal)?;
            let mut recorded = record(&table[row.0 as usize], runs, source);
            let mut distinct = FxHashSet::default();
            for occurrence in &mut recorded.reads {
                let read = if let Some(read) = tokens.get(&occurrence.token) {
                    *read
                } else {
                    let token = self.tokens.len() as u32;
                    self.tokens.push(occurrence.token.clone());
                    let read = builder
                        .add_node(VaporEffectNode::Read { token })
                        .map_err(storage_refusal)?;
                    tokens.insert(occurrence.token.clone(), read);
                    read
                };
                occurrence.read = Some(read);
                if distinct.insert(read) {
                    builder
                        .add_edge(effect, read, ())
                        .map_err(storage_refusal)?;
                }
            }
            self.records[row.0 as usize] = Some(recorded);
        }
        Ok(group)
    }
    pub(super) fn finish(&mut self) {
        self.graph = Some(
            self.builder
                .take()
                .expect("effect graph closes once")
                .finish(),
        );
    }
    pub(super) fn relations(&self) -> Result<&EffectRelations<'c>, VueTargetRefusal> {
        self.graph
            .as_ref()
            .ok_or_else(|| refusal("unsealed-effect-graph"))
    }
    pub(super) fn record_work<M: ObserveExec>(&self, ledger: &mut CompilerWorkLedger<M>) {
        if let Some(graph) = &self.graph {
            ledger.record(WorkKind::PlanningEffectNodes, graph.node_count() as u64);
            ledger.record(WorkKind::PlanningEffectEdges, graph.edge_count() as u64);
        }
    }
}
pub(super) fn effect_row(
    graph: &EffectRelations<'_>,
    node: Id<VaporEffectKey>,
) -> Result<EffectId, VueTargetRefusal> {
    match graph.node(node) {
        Some(VaporEffectNode::Effect { row }) => Ok(*row),
        _ => Err(refusal("invalid-effect-group-edge")),
    }
}
fn storage_refusal(_: StructureError) -> VueTargetRefusal {
    refusal("effect-graph-storage")
}
fn refusal(code: &'static str) -> VueTargetRefusal {
    VueTargetRefusal::Unsupported {
        code,
        span: Span::new(0, 0),
    }
}
