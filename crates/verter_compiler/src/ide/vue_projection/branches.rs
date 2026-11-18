#![allow(dead_code)]
//! Ordered template blocks and the guards that dominate each one.

use rustc_hash::FxHashMap;

use crate::framework_common::projection_plan::{
    AdmittedExpressionId, ComponentUseId, ElementUseId, ExpressionKind, LexicalScopeId,
    PlanSnapshotId, ProjectionPlan,
};
use crate::ide::vue_projection::checking_fragment::{
    BlockHead, CheckingFragment, ObligationFamily, PlacedStatement, StatementAnchor,
    StatementPhase, TemplateBlockId, UseHost,
};
// The canonical template topology (`template/topology/`). Read through its
// accessors only; this module adds no template walker of its own.
use crate::template::topology::{
    RegionCompleteness, VueBranchRole, VueRegionId, VueRegionKind, VueTemplateNodeId,
    VueTemplateNodeKind, VueTemplateTopology,
};

/// What opens a block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TemplateBlockKind {
    /// The template root.
    Root,
    /// One arm of an if / else-if / else chain. `condition` is `None` for else.
    Arm {
        chain: u32,
        ordinal: u32,
        condition: Option<AdmittedExpressionId>,
    },
    /// A `v-for` body.
    Loop { source: AdmittedExpressionId },
    /// A slot body provided to a component use. `slot` is the index among
    /// the component's slot bodies; `template` is the parser node that
    /// provides it: the `<template>` element, or the component's own node
    /// for a tag-level `v-slot`. `None` for the implicit default slot.
    SlotBody {
        use_id: ComponentUseId,
        slot: u32,
        template: Option<u32>,
    },
}

/// One thing inside a block, in authored order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TemplateItem {
    Interpolation(AdmittedExpressionId),
    Component(ComponentUseId),
    Element(ElementUseId),
    Block(TemplateBlockId),
}

/// One block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateBlock {
    pub id: TemplateBlockId,
    pub parent: Option<TemplateBlockId>,
    pub kind: TemplateBlockKind,
    pub region: VueRegionId,
    pub lexical_env: LexicalScopeId,
    pub items: Vec<TemplateItem>,
    /// False when the topology marks the region incomplete.
    pub complete: bool,
}

/// Whether a guard holds or fails on the way into a block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardPolarity {
    Holds,
    Fails,
}

/// One guard on a path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardStep {
    pub expression: AdmittedExpressionId,
    pub polarity: GuardPolarity,
}

/// Guards from the root to a block: ancestors first; within one chain the
/// preceding siblings as `Fails` in authored order, then the arm's own
/// condition as `Holds`. Never sorted, never rebuilt from text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DominanceOrderedBranchPath {
    pub block: TemplateBlockId,
    pub steps: Vec<GuardStep>,
}

/// Runtime-root syntax retained during the existing topology pass. Only the
/// synthetic root and transparent template wrappers have inventoried children;
/// native and component descendants cannot become additional runtime roots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TemplateRootNode {
    pub kind: VueTemplateNodeKind,
    pub host: Option<UseHost>,
    pub native: Option<(
        String,
        crate::framework_common::projection_plan::ElementNamespace,
    )>,
    pub children: Vec<usize>,
    pub branch: Option<(u32, VueBranchRole)>,
    pub repeated: bool,
}

/// Blocks and paths of one plan snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateBranchProjection {
    pub snapshot: PlanSnapshotId,
    pub complete: bool,
    /// Indexed by `TemplateBlockId`; a parent precedes its children.
    pub blocks: Vec<TemplateBlock>,
    /// Indexed by `TemplateBlockId`.
    pub paths: Vec<DominanceOrderedBranchPath>,
    runtime_roots: Vec<TemplateRootNode>,
}

impl TemplateBranchProjection {
    /// The product before blocks are projected: no block, no path, and
    /// never complete.
    #[must_use]
    pub fn pending(snapshot: PlanSnapshotId) -> Self {
        Self {
            snapshot,
            complete: false,
            blocks: Vec::new(),
            paths: Vec::new(),
            runtime_roots: Vec::new(),
        }
    }
    #[must_use]
    pub(crate) fn runtime_roots(&self) -> &[TemplateRootNode] {
        &self.runtime_roots
    }
    #[must_use]
    pub fn root(&self) -> Option<&TemplateBlock> {
        self.blocks.first()
    }
    #[must_use]
    pub fn block(&self, id: TemplateBlockId) -> Option<&TemplateBlock> {
        self.blocks.get(id.0 as usize)
    }
    #[must_use]
    pub fn path(&self, id: TemplateBlockId) -> Option<&DominanceOrderedBranchPath> {
        self.paths.get(id.0 as usize)
    }
    /// Block and item index that hold `item`.
    #[must_use]
    pub fn place_of(&self, item: &TemplateItem) -> Option<(TemplateBlockId, u32)> {
        self.blocks.iter().find_map(|block| {
            block
                .items
                .iter()
                .position(|held| held == item)
                .map(|index| (block.id, index as u32))
        })
    }
    /// `if (` cond `) {`, `else if (` cond `) {` or `else {` for an `Arm`
    /// block; `None` for other kinds. The first arm of a chain opens with
    /// `if` whatever its directive, so a chain the parser starts at a
    /// `v-else-if` still checks its condition; a leading else is a plain
    /// block.
    #[must_use]
    pub fn arm_head(&self, id: TemplateBlockId) -> Option<BlockHead> {
        let TemplateBlockKind::Arm {
            ordinal, condition, ..
        } = &self.block(id)?.kind
        else {
            return None;
        };
        let mut open = CheckingFragment::new();
        match (condition, *ordinal) {
            (Some(condition), 0) => {
                open.push_synth("if (")
                    .push_expression(condition.clone())
                    .push_synth(") {\n");
            }
            (Some(condition), _) => {
                open.push_synth("else if (")
                    .push_expression(condition.clone())
                    .push_synth(") {\n");
            }
            // An else the parser found no condition before is a plain block.
            (None, 0) => {
                open.push_synth("{\n");
            }
            (None, _) => {
                open.push_synth("else {\n");
            }
        }
        Some(BlockHead {
            before: CheckingFragment::new(),
            open,
        })
    }
    /// One `(expr);` statement per interpolation item.
    #[must_use]
    pub fn statements(&self) -> Vec<PlacedStatement> {
        let mut statements = Vec::new();
        for block in &self.blocks {
            for (index, item) in block.items.iter().enumerate() {
                let TemplateItem::Interpolation(expression) = item else {
                    continue;
                };
                let mut fragment = CheckingFragment::synth("(");
                fragment
                    .push_expression(expression.clone())
                    .push_synth(");\n");
                statements.push(PlacedStatement {
                    anchor: StatementAnchor::Item {
                        block: block.id,
                        item: index as u32,
                    },
                    phase: StatementPhase::Main,
                    family: ObligationFamily::Interpolation,
                    host: None,
                    fragment,
                });
            }
        }
        statements
    }
}

/// Join the plan's expressions, uses and elements to the canonical topology.
///
/// A block opens for the root and for each `v-if` arm, `v-for` body and slot
/// body region, at the first node of the region in preorder, as an item of
/// the enclosing block. Each plan expression, use and element joins its
/// node through `node_for_syntax` and lands in the innermost block around
/// it. A region whose head the plan did not admit (a condition, a loop
/// source, a slot's use) is left out with everything inside it, and the
/// projection is then incomplete.
#[must_use]
pub fn project_template_branches(
    plan: &ProjectionPlan,
    topology: &VueTemplateTopology<'_>,
) -> TemplateBranchProjection {
    let joins = PlanJoins::of(plan);
    let mut builder = Builder {
        plan,
        topology,
        joins: &joins,
        blocks: Vec::new(),
        opened: FxHashMap::default(),
        broken_chains: Vec::new(),
        root_parents: topology
            .children(topology.root())
            .map(|node| (node, 0))
            .collect(),
        runtime_roots: vec![TemplateRootNode {
            kind: VueTemplateNodeKind::Root,
            host: None,
            native: None,
            children: Vec::new(),
            branch: None,
            repeated: false,
        }],
        complete: true,
    };
    for node in topology.nodes() {
        builder.visit(node);
    }
    builder.finish()
}

/// The plan's facts keyed by the parser node that carries them.
struct PlanJoins<'p> {
    interpolations: FxHashMap<u32, Vec<&'p AdmittedExpressionId>>,
    conditions: FxHashMap<u32, &'p AdmittedExpressionId>,
    sources: FxHashMap<u32, &'p AdmittedExpressionId>,
    loop_scopes: FxHashMap<u32, &'p LexicalScopeId>,
    uses: FxHashMap<u32, (&'p ComponentUseId, &'p LexicalScopeId)>,
    elements: FxHashMap<u32, (&'p ElementUseId, &'p LexicalScopeId)>,
}

impl<'p> PlanJoins<'p> {
    fn of(plan: &'p ProjectionPlan) -> Self {
        let mut joins = Self {
            interpolations: FxHashMap::default(),
            conditions: FxHashMap::default(),
            sources: FxHashMap::default(),
            loop_scopes: FxHashMap::default(),
            uses: FxHashMap::default(),
            elements: FxHashMap::default(),
        };
        for occurrence in plan.expressions() {
            let node = occurrence.syntax_node;
            match occurrence.kind {
                ExpressionKind::Interpolation => joins
                    .interpolations
                    .entry(node)
                    .or_default()
                    .push(&occurrence.id),
                ExpressionKind::BranchCondition => {
                    joins.conditions.entry(node).or_insert(&occurrence.id);
                }
                ExpressionKind::VForSource => {
                    joins.sources.entry(node).or_insert(&occurrence.id);
                }
                ExpressionKind::VForAlias => {
                    joins
                        .loop_scopes
                        .entry(node)
                        .or_insert(&occurrence.lexical_env);
                }
                _ => {}
            }
        }
        for component in &plan.uses {
            joins.uses.insert(
                component.syntax_node,
                (&component.id, &component.lexical_env),
            );
        }
        for element in &plan.elements {
            joins
                .elements
                .insert(element.syntax_node, (&element.id, &element.lexical_env));
        }
        joins
    }
}

/// A block while its scope is still being learnt.
struct Draft {
    parent: Option<TemplateBlockId>,
    kind: TemplateBlockKind,
    region: VueRegionId,
    /// Scope the block itself opens (a loop or slot scope); `None` for a
    /// block that reads its parent's scope.
    own_scope: Option<LexicalScopeId>,
    items: Vec<TemplateItem>,
    complete: bool,
}

struct Builder<'p, 't> {
    plan: &'p ProjectionPlan,
    topology: &'t VueTemplateTopology<'t>,
    joins: &'t PlanJoins<'p>,
    blocks: Vec<Draft>,
    /// Region to its block; `None` for a region left out.
    opened: FxHashMap<VueRegionId, Option<TemplateBlockId>>,
    /// If chains with an arm left out: the whole chain is incomplete, so no
    /// later arm becomes the else of a different condition.
    broken_chains: Vec<u32>,
    root_parents: FxHashMap<VueTemplateNodeId, usize>,
    runtime_roots: Vec<TemplateRootNode>,
    complete: bool,
}

impl Builder<'_, '_> {
    fn syntax(&self, node: VueTemplateNodeId) -> Option<u32> {
        self.topology.syntax(node).map(|syntax| syntax.0 as u32)
    }

    /// Open the blocks around `node`, outermost first, then add its items to
    /// the innermost one.
    fn visit(&mut self, node: VueTemplateNodeId) {
        self.retain_runtime_root(node);
        let mut regions: Vec<VueRegionId> = self
            .topology
            .enclosing_regions(node)
            .filter(|&region| {
                !matches!(
                    self.topology.region_kind(region),
                    VueRegionKind::ComponentChildren
                )
            })
            .collect();
        regions.reverse();
        let mut block = None;
        for region in regions {
            block = match self.opened.get(&region) {
                Some(known) => *known,
                None => {
                    let opened = match (block, self.topology.region_kind(region)) {
                        (_, VueRegionKind::Root) => self.open_root(region),
                        (Some(parent), _) => self.open(region, node, parent),
                        (None, _) => None,
                    };
                    self.opened.insert(region, opened);
                    opened
                }
            };
            if block.is_none() {
                return;
            }
        }
        let (Some(block), Some(syntax)) = (block, self.syntax(node)) else {
            return;
        };
        let items = &mut self.blocks[block.0 as usize].items;
        if let Some(expressions) = self.joins.interpolations.get(&syntax) {
            items.extend(
                expressions
                    .iter()
                    .map(|&id| TemplateItem::Interpolation(id.clone())),
            );
        }
        if let Some((id, _)) = self.joins.uses.get(&syntax) {
            items.push(TemplateItem::Component((*id).clone()));
        }
        if let Some((id, _)) = self.joins.elements.get(&syntax) {
            items.push(TemplateItem::Element((*id).clone()));
        }
    }

    fn retain_runtime_root(&mut self, node: VueTemplateNodeId) {
        let Some(parent) = self.root_parents.remove(&node) else {
            return;
        };
        let kind = self.topology.kind(node);
        let syntax = self.syntax(node);
        if kind == VueTemplateNodeKind::Text
            && syntax.is_some_and(|syntax| {
                matches!(&self.topology.ast().nodes[syntax as usize].kind,
                crate::ast::types::AstNodeKind::Text(text) if text.is_whitespace_only)
            })
        {
            return;
        }
        let host = syntax.and_then(|syntax| {
            self.joins
                .uses
                .get(&syntax)
                .map(|(id, _)| UseHost::Component((*id).clone()))
                .or_else(|| {
                    self.joins
                        .elements
                        .get(&syntax)
                        .map(|(id, _)| UseHost::Element((*id).clone()))
                })
        });
        let native = (kind == VueTemplateNodeKind::Element).then(|| {
            use crate::framework_common::projection_plan::{element_namespace, open_tag_name_span};
            let element = self.topology.element(node).expect("element topology node");
            let (_, _, tag) = open_tag_name_span(&element.tag_open, self.topology.source());
            (tag.to_string(), element_namespace(tag, &[]))
        });
        let branch = self.topology.chain_of(node).map(|(chain, ordinal)| {
            (
                chain.index() as u32,
                self.topology.branches(chain)[ordinal as usize].role,
            )
        });
        let index = self.runtime_roots.len();
        self.runtime_roots.push(TemplateRootNode {
            kind,
            host,
            native,
            children: Vec::new(),
            branch,
            repeated: self.topology.for_region(node).is_some(),
        });
        self.runtime_roots[parent].children.push(index);
        if kind == VueTemplateNodeKind::Template {
            self.root_parents
                .extend(self.topology.children(node).map(|child| (child, index)));
        }
    }

    fn open_root(&mut self, region: VueRegionId) -> Option<TemplateBlockId> {
        Some(self.push(None, TemplateBlockKind::Root, region, None))
    }

    /// Open the block of `region`, first reached at `node`, inside `parent`.
    fn open(
        &mut self,
        region: VueRegionId,
        node: VueTemplateNodeId,
        parent: TemplateBlockId,
    ) -> Option<TemplateBlockId> {
        let Some((kind, own_scope)) = self.head(region, node) else {
            self.complete = false;
            if self.topology.region_kind(region) == VueRegionKind::IfBranch {
                if let Some((chain, _)) = self.topology.chain_of(node) {
                    self.broken_chains.push(chain.index() as u32);
                }
            }
            return None;
        };
        let id = self.push(Some(parent), kind, region, own_scope);
        self.blocks[parent.0 as usize]
            .items
            .push(TemplateItem::Block(id));
        Some(id)
    }

    /// What opens the block of `region`, first reached at `node`, and the
    /// scope it opens; `None` when the plan did not admit its head.
    fn head(
        &self,
        region: VueRegionId,
        node: VueTemplateNodeId,
    ) -> Option<(TemplateBlockKind, Option<LexicalScopeId>)> {
        let topology = self.topology;
        match topology.region_kind(region) {
            VueRegionKind::IfBranch => {
                let (chain, ordinal) = topology.chain_of(node)?;
                if self.broken_chains.contains(&(chain.index() as u32)) {
                    return None;
                }
                let condition = match self.syntax(node) {
                    Some(syntax) => self.joins.conditions.get(&syntax).map(|id| (*id).clone()),
                    None => None,
                };
                let role = topology.branches(chain)[ordinal as usize].role;
                if condition.is_none() && role != VueBranchRole::Else {
                    None
                } else {
                    let kind = TemplateBlockKind::Arm {
                        chain: chain.index() as u32,
                        ordinal,
                        condition,
                    };
                    Some((kind, None))
                }
            }
            VueRegionKind::ForBody => {
                let syntax = self.syntax(node)?;
                self.joins.sources.get(&syntax).map(|source| {
                    (
                        TemplateBlockKind::Loop {
                            source: (*source).clone(),
                        },
                        self.joins
                            .loop_scopes
                            .get(&syntax)
                            .map(|env| (*env).clone()),
                    )
                })
            }
            VueRegionKind::SlotBody => self.slot_body(region, node),
            VueRegionKind::Root | VueRegionKind::ComponentChildren => None,
        }
    }

    /// The slot body `region` holds: the component use that receives it and
    /// the slot's index among that component's slot bodies. The slot's
    /// parameters are admitted in the enclosing scope, so the scope the body
    /// reads is learnt from its items when the projection finishes.
    fn slot_body(
        &self,
        region: VueRegionId,
        node: VueTemplateNodeId,
    ) -> Option<(TemplateBlockKind, Option<LexicalScopeId>)> {
        let topology = self.topology;
        let (component, slot, row) =
            std::iter::successors(topology.parent(node), |&up| topology.parent(up)).find_map(
                |candidate| {
                    let rows = topology.slot_regions(candidate);
                    rows.iter()
                        .position(|slot| slot.region == region)
                        .map(|index| (candidate, index as u32, rows[index]))
                },
            )?;
        let component_syntax = self.syntax(component)?;
        let (use_id, _) = self.joins.uses.get(&component_syntax)?;
        let template = match row.template {
            Some(template) => Some(self.syntax(template)?),
            None if row.implicit_default => None,
            None => Some(component_syntax),
        };
        Some((
            TemplateBlockKind::SlotBody {
                use_id: (*use_id).clone(),
                slot,
                template,
            },
            None,
        ))
    }

    fn push(
        &mut self,
        parent: Option<TemplateBlockId>,
        kind: TemplateBlockKind,
        region: VueRegionId,
        own_scope: Option<LexicalScopeId>,
    ) -> TemplateBlockId {
        let complete = self.topology.region(region).completeness != RegionCompleteness::Incomplete;
        if !complete {
            self.complete = false;
        }
        let id = TemplateBlockId(self.blocks.len() as u32);
        self.blocks.push(Draft {
            parent,
            kind,
            region,
            own_scope,
            items: Vec::new(),
            complete,
        });
        id
    }

    /// The scope a block's own items are read in, learnt from them: an
    /// interpolation, use or element reads it, an arm condition or a loop
    /// source is evaluated in it, and a slot body's component is used in it.
    fn items_scope(&self, block: usize) -> Option<LexicalScopeId> {
        let plan = self.plan;
        self.blocks[block].items.iter().find_map(|item| match item {
            TemplateItem::Interpolation(id) => plan
                .expression(id)
                .map(|occurrence| occurrence.lexical_env.clone()),
            TemplateItem::Component(id) => plan
                .uses
                .iter()
                .find(|component| component.id == *id)
                .map(|component| component.lexical_env.clone()),
            TemplateItem::Element(id) => plan
                .elements
                .iter()
                .find(|element| element.id == *id)
                .map(|element| element.lexical_env.clone()),
            TemplateItem::Block(child) => match &self.blocks[child.0 as usize].kind {
                TemplateBlockKind::Arm {
                    condition: Some(condition),
                    ..
                } => plan
                    .expression(condition)
                    .map(|occurrence| occurrence.lexical_env.clone()),
                TemplateBlockKind::Arm {
                    condition: None, ..
                } => self.items_scope(child.0 as usize),
                TemplateBlockKind::Loop { source } => plan
                    .expression(source)
                    .map(|occurrence| occurrence.lexical_env.clone()),
                TemplateBlockKind::SlotBody { use_id, .. } => plan
                    .uses
                    .iter()
                    .find(|component| component.id == *use_id)
                    .map(|component| component.lexical_env.clone()),
                TemplateBlockKind::Root => None,
            },
        })
    }

    fn finish(self) -> TemplateBranchProjection {
        let snapshot = self.plan.snapshot.clone();
        // A template with no expression has nothing to check.
        let Some(root_scope) = (!self.blocks.is_empty())
            .then(|| self.items_scope(0))
            .flatten()
        else {
            return TemplateBranchProjection {
                snapshot,
                complete: self.complete,
                blocks: Vec::new(),
                paths: Vec::new(),
                runtime_roots: self.runtime_roots,
            };
        };
        let mut blocks: Vec<TemplateBlock> = Vec::with_capacity(self.blocks.len());
        let mut paths: Vec<DominanceOrderedBranchPath> = Vec::with_capacity(self.blocks.len());
        for (index, draft) in self.blocks.iter().enumerate() {
            let id = TemplateBlockId(index as u32);
            let inherited = draft.parent.map_or_else(
                || root_scope.clone(),
                |parent| blocks[parent.0 as usize].lexical_env.clone(),
            );
            let lexical_env = match draft.kind {
                TemplateBlockKind::SlotBody { .. } => self.items_scope(index),
                _ => draft.own_scope.clone(),
            }
            .unwrap_or(inherited);
            let mut steps = draft
                .parent
                .map(|parent| paths[parent.0 as usize].steps.clone())
                .unwrap_or_default();
            if let (
                TemplateBlockKind::Arm {
                    chain,
                    ordinal,
                    condition,
                },
                Some(parent),
            ) = (&draft.kind, draft.parent)
            {
                for sibling in &blocks[parent.0 as usize].items {
                    let TemplateItem::Block(sibling) = sibling else {
                        continue;
                    };
                    if let TemplateBlockKind::Arm {
                        chain: sibling_chain,
                        ordinal: sibling_ordinal,
                        condition: Some(sibling_condition),
                    } = &self.blocks[sibling.0 as usize].kind
                    {
                        if sibling_chain == chain && sibling_ordinal < ordinal {
                            steps.push(GuardStep {
                                expression: sibling_condition.clone(),
                                polarity: GuardPolarity::Fails,
                            });
                        }
                    }
                }
                if let Some(condition) = condition {
                    steps.push(GuardStep {
                        expression: condition.clone(),
                        polarity: GuardPolarity::Holds,
                    });
                }
            }
            let broken = matches!(&draft.kind,
                TemplateBlockKind::Arm { chain, .. } if self.broken_chains.contains(chain));
            blocks.push(TemplateBlock {
                id,
                parent: draft.parent,
                kind: draft.kind.clone(),
                region: draft.region,
                lexical_env,
                items: draft.items.clone(),
                complete: draft.complete && !broken,
            });
            paths.push(DominanceOrderedBranchPath { block: id, steps });
        }
        TemplateBranchProjection {
            snapshot,
            complete: self.complete,
            blocks,
            paths,
            runtime_roots: self.runtime_roots,
        }
    }
}
