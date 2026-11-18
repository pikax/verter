use verter_audit::observe::ObserveExec;
use verter_span::Span;

use super::{
    RegionCompleteness, SyntaxNodeId, VueBranchRole, VueForRegion, VueIfBranch, VueIfBranchKey,
    VueIfChain, VueIfChainId, VueIfChainKey, VueRegion, VueRegionKind, VueSlotRegion,
    VueTemplateNode, VueTemplateNodeKey, VueTemplateNodeKind, VueTemplateTopology,
};
use crate::ast::types::{
    AstNodeKind, ConditionalChain, ElementNode, ElementNodeConditionKind, TagType, TemplateAst,
};
use crate::framework_common::structure::{Arena, FlatArena, Structure, StructureError};
use crate::work_ledger::{CompilerWorkLedger, WorkKind};

/// Parser recovery evidence for the template.
#[derive(Debug, Clone, Copy, Default)]
pub struct TopologyRecoveryFacts<'a> {
    /// Spans of template parse errors, in any order.
    pub error_spans: &'a [Span],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopologyBuildError {
    Storage(StructureError),
    /// A parser node index is out of range or reached twice.
    InconsistentSyntaxTree,
}

impl From<StructureError> for TopologyBuildError {
    fn from(error: StructureError) -> Self {
        Self::Storage(error)
    }
}

/// Build the topology in one preorder pass over the parsed template. Visits
/// each parser node once and copies no text.
pub fn build_template_topology<'p, M: ObserveExec>(
    source: &'p str,
    ast: &'p TemplateAst,
    recovery: TopologyRecoveryFacts<'_>,
    ledger: &mut CompilerWorkLedger<M>,
) -> Result<VueTemplateTopology<'p>, TopologyBuildError> {
    let topology = Builder::new(source, ast, recovery).build()?;
    ledger.record(WorkKind::StructureSourceSizedVisits, 1);
    ledger.record(WorkKind::StructureTopologyNodes, topology.len() as u64);
    ledger.record(
        WorkKind::StructureRegions,
        topology.regions().count() as u64,
    );
    Ok(topology)
}

type Tree<'p> = Structure<'p, VueTemplateNodeKey, VueRegion, VueTemplateNode>;

const NONE: u32 = u32::MAX;

/// A parser node waiting to be recorded, with everything decided about it
/// by its parent. Indices are positions in the build records.
struct Pending {
    syntax: SyntaxNodeId,
    parent: u32,
    region: u32,
    branch: Option<PendingBranch>,
    for_region: Option<u32>,
    /// The component this node is a slot template of.
    slot_of: Option<u32>,
}

#[derive(Clone, Copy)]
struct PendingBranch {
    chain: u32,
    index: u32,
    role: VueBranchRole,
    region: u32,
}

struct NodeRecord {
    parent: u32,
    region: u32,
    span: Span,
    data: VueTemplateNode,
}

struct RegionRecord {
    parent: u32,
    owner: u32,
    kind: VueRegionKind,
    completeness: RegionCompleteness,
}

struct BranchRecord {
    chain: u32,
    region: u32,
    node: u32,
    role: VueBranchRole,
}

struct SlotRecord {
    /// Source start of the slot, to order slots of one component.
    start: u32,
    region: u32,
    component: u32,
    template: Option<u32>,
    implicit_default: bool,
}

/// Error evidence widened to half-open ranges, sorted by start, with the
/// running maximum of the ends, so each node asks one binary search.
struct ErrorIndex {
    starts: Vec<u32>,
    max_end: Vec<u32>,
}

impl ErrorIndex {
    fn new(spans: &[Span]) -> Self {
        let mut ranges: Vec<(u32, u32)> = spans
            .iter()
            .map(|s| (s.start, s.end.max(s.start.saturating_add(1))))
            .collect();
        ranges.sort_unstable();
        let mut high = 0;
        let max_end = ranges
            .iter()
            .map(|&(_, end)| {
                high = high.max(end);
                high
            })
            .collect();
        Self {
            starts: ranges.into_iter().map(|(start, _)| start).collect(),
            max_end,
        }
    }

    fn touches(&self, span: Span) -> bool {
        let end = span.end.max(span.start.saturating_add(1));
        let before = self.starts.partition_point(|&start| start < end);
        before > 0 && self.max_end[before - 1] > span.start
    }
}

/// Records the template in one preorder pass, settles every region's final
/// marking and table index, then replays the records into the frozen
/// storage: a region's payload is immutable once it is stored, and its
/// completeness depends on nodes that come after it.
struct Builder<'p> {
    source: &'p str,
    ast: &'p TemplateAst,
    errors: ErrorIndex,
    nodes: Vec<NodeRecord>,
    regions: Vec<RegionRecord>,
    /// Parser index to node index; `NONE` for nodes never reached.
    by_syntax: Vec<u32>,
    /// Owner node of each chain, in creation order.
    chain_owners: Vec<u32>,
    /// In node order, so each chain's rows are in source order.
    branch_rows: Vec<BranchRecord>,
    /// `(node, chain, index in chain)` in node order.
    chain_of_rows: Vec<(u32, u32, u32)>,
    /// `(region, node)` in node order.
    for_rows: Vec<(u32, u32)>,
    slot_rows: Vec<SlotRecord>,
    stack: Vec<Pending>,
    scratch: Vec<Pending>,
    /// Per child of the node being expanded: its chain and index in it.
    membership: Vec<Option<(u32, u32)>>,
}

impl<'p> Builder<'p> {
    fn new(source: &'p str, ast: &'p TemplateAst, recovery: TopologyRecoveryFacts<'_>) -> Self {
        Self {
            source,
            ast,
            errors: ErrorIndex::new(recovery.error_spans),
            nodes: Vec::new(),
            regions: Vec::new(),
            by_syntax: vec![NONE; ast.nodes.len()],
            chain_owners: Vec::new(),
            branch_rows: Vec::new(),
            chain_of_rows: Vec::new(),
            for_rows: Vec::new(),
            slot_rows: Vec::new(),
            stack: Vec::new(),
            scratch: Vec::new(),
            membership: Vec::new(),
        }
    }

    fn build(mut self) -> Result<VueTemplateTopology<'p>, TopologyBuildError> {
        if u32::try_from(self.ast.nodes.len()).is_err() {
            return Err(StructureError::Capacity.into());
        }
        let content = self.ast.root.content.as_ref();
        let root_span = content.map_or(Span::new(0, 0), |c| Span::new(c.start, c.end));
        let root_region = self.add_region(NONE, NONE, VueRegionKind::Root);
        self.nodes.push(NodeRecord {
            parent: NONE,
            region: root_region,
            span: root_span,
            data: VueTemplateNode {
                kind: VueTemplateNodeKind::Root,
                syntax: u32::MAX,
            },
        });
        self.mark(root_span, root_region);
        if let Some(content) = content {
            self.expand(
                0,
                root_region,
                None,
                None,
                &content.children,
                &content.v_if_chains,
            )?;
        }
        while let Some(pending) = self.stack.pop() {
            self.visit(pending)?;
        }
        self.freeze()
    }

    fn visit(&mut self, pending: Pending) -> Result<(), TopologyBuildError> {
        let ast = self.ast;
        let node = ast
            .nodes
            .get(pending.syntax.0)
            .ok_or(TopologyBuildError::InconsistentSyntaxTree)?;
        let (kind, span, element) = match &node.kind {
            AstNodeKind::Element(element) => {
                let kind = match element.tag_type {
                    TagType::Element => VueTemplateNodeKind::Element,
                    TagType::Component => VueTemplateNodeKind::Component,
                    TagType::SlotOutlet => VueTemplateNodeKind::SlotOutlet,
                    TagType::Template => VueTemplateNodeKind::Template,
                };
                (kind, element_span(element), Some(&**element))
            }
            AstNodeKind::Text(text) => (
                VueTemplateNodeKind::Text,
                Span::new(text.start, text.end),
                None,
            ),
            AstNodeKind::Comment(comment) => (
                VueTemplateNodeKind::Comment,
                Span::new(comment.start, comment.end),
                None,
            ),
            AstNodeKind::Interpolation(interpolation) => (
                VueTemplateNodeKind::Interpolation,
                Span::new(interpolation.start, interpolation.end),
                None,
            ),
        };
        if self.by_syntax[pending.syntax.0] != NONE {
            return Err(TopologyBuildError::InconsistentSyntaxTree);
        }
        let id = u32::try_from(self.nodes.len()).map_err(|_| StructureError::Capacity)?;
        self.by_syntax[pending.syntax.0] = id;
        self.nodes.push(NodeRecord {
            parent: pending.parent,
            region: pending.region,
            span,
            data: VueTemplateNode {
                kind,
                syntax: pending.syntax.0 as u32,
            },
        });
        self.mark(span, pending.region);
        if element.is_some_and(is_unclosed) {
            self.raise(pending.region, RegionCompleteness::Incomplete);
        }
        if let Some(branch) = pending.branch {
            self.branch_rows.push(BranchRecord {
                chain: branch.chain,
                region: branch.region,
                node: id,
                role: branch.role,
            });
            self.chain_of_rows.push((id, branch.chain, branch.index));
        }
        if let Some(region) = pending.for_region {
            self.for_rows.push((region, id));
        }
        if let Some(element) = element {
            match &element.content {
                Some(content) => self.expand(
                    id,
                    pending.region,
                    Some(element),
                    pending.slot_of,
                    &content.children,
                    &content.v_if_chains,
                )?,
                // A slot template still declares its slot with no body.
                None if pending.slot_of.is_some() => {
                    self.expand(id, pending.region, Some(element), pending.slot_of, &[], &[])?
                }
                None => {}
            }
        }
        Ok(())
    }

    /// Decide the region, branch and loop membership of every child of
    /// `node`, then queue the children so they pop in source order.
    fn expand(
        &mut self,
        node: u32,
        node_region: u32,
        element: Option<&'p ElementNode>,
        slot_of: Option<u32>,
        children: &[crate::types::NodeId],
        chains: &[ConditionalChain],
    ) -> Result<(), TopologyBuildError> {
        let ast = self.ast;
        let is_component = element.is_some_and(|e| e.tag_type == TagType::Component);
        let start = element.map_or(0, |e| e.tag_open.start);
        // The region every child sits in before its own `v-if` / `v-for`.
        // Components without a slot of their own split their children
        // between the slot templates and the implicit default slot.
        let mut fixed = Some(node_region);
        if is_component && element.is_some_and(|e| e.v_slot.is_some()) {
            fixed = Some(self.slot_body(node_region, node, node, None, false, start));
        } else if let Some(component) = slot_of {
            fixed = Some(self.slot_body(node_region, node, component, Some(node), false, start));
        } else if is_component {
            fixed = None;
        }
        // Content the parser discarded leaves no child node to carry the
        // recovery evidence, so a slot body is marked by the content extent.
        if let (Some(region), Some(content)) = (
            fixed.filter(|&region| region != node_region),
            element.and_then(|e| e.content.as_ref()),
        ) {
            self.mark(Span::new(content.start, content.end), region);
        }
        let splits_slots = fixed.is_none();
        let has_slot_templates =
            splits_slots && children.iter().any(|&child| is_slot_template(ast, child));
        let mut component_children = None;
        let mut default_slot = None;

        self.membership.clear();
        self.membership.resize(children.len(), None);
        for chain in chains {
            let number = self.new_chain(node);
            for (index, &member) in chain.member_indices.iter().enumerate() {
                let slot = self
                    .membership
                    .get_mut(member)
                    .ok_or(TopologyBuildError::InconsistentSyntaxTree)?;
                *slot = Some((number, index as u32));
            }
        }
        self.scratch.clear();
        for (position, &child) in children.iter().enumerate() {
            let child_node = ast
                .nodes
                .get(child.0)
                .ok_or(TopologyBuildError::InconsistentSyntaxTree)?;
            let slot_template = splits_slots && is_slot_template(ast, child);
            let base = match fixed {
                Some(region) => region,
                None if slot_template || (has_slot_templates && is_ignorable(&child_node.kind)) => {
                    match component_children {
                        Some(region) => region,
                        None => {
                            let region = self.add_region(
                                node_region,
                                node,
                                VueRegionKind::ComponentChildren,
                            );
                            component_children = Some(region);
                            region
                        }
                    }
                }
                None => match default_slot {
                    Some(region) => region,
                    None => {
                        let start = node_span_start(&child_node.kind);
                        let region = self.slot_body(node_region, node, node, None, true, start);
                        default_slot = Some(region);
                        region
                    }
                },
            };

            let mut region = base;
            let mut branch = None;
            let mut for_region = None;
            if let AstNodeKind::Element(child_element) = &child_node.kind {
                if let Some(condition) = &child_element.v_condition {
                    let role = match condition.kind {
                        ElementNodeConditionKind::If => VueBranchRole::If,
                        ElementNodeConditionKind::ElseIf => VueBranchRole::ElseIf,
                        ElementNodeConditionKind::Else => VueBranchRole::Else,
                    };
                    // A conditional sibling the parser put in no chain has
                    // nothing to continue: its own chain, and its region
                    // cannot be trusted.
                    let (chain, index, orphan) = match self.membership[position] {
                        Some((chain, index)) => (chain, index, false),
                        None => (self.new_chain(node), 0, true),
                    };
                    let branch_region = self.add_region(base, node, VueRegionKind::IfBranch);
                    if orphan {
                        self.raise(branch_region, RegionCompleteness::Recovered);
                    }
                    region = branch_region;
                    branch = Some(PendingBranch {
                        chain,
                        index,
                        role,
                        region: branch_region,
                    });
                }
                if child_element.v_for.is_some() {
                    let body = self.add_region(region, node, VueRegionKind::ForBody);
                    region = body;
                    for_region = Some(body);
                }
            }
            self.scratch.push(Pending {
                syntax: child,
                parent: node,
                region,
                branch,
                for_region,
                slot_of: slot_template.then_some(node),
            });
        }
        self.stack.extend(self.scratch.drain(..).rev());
        Ok(())
    }

    fn new_chain(&mut self, owner: u32) -> u32 {
        self.chain_owners.push(owner);
        (self.chain_owners.len() - 1) as u32
    }

    fn add_region(&mut self, parent: u32, owner: u32, kind: VueRegionKind) -> u32 {
        self.regions.push(RegionRecord {
            parent,
            owner,
            kind,
            completeness: RegionCompleteness::Complete,
        });
        (self.regions.len() - 1) as u32
    }

    fn slot_body(
        &mut self,
        parent: u32,
        owner: u32,
        component: u32,
        template: Option<u32>,
        implicit_default: bool,
        start: u32,
    ) -> u32 {
        let region = self.add_region(parent, owner, VueRegionKind::SlotBody);
        self.slot_rows.push(SlotRecord {
            start,
            region,
            component,
            template,
            implicit_default,
        });
        region
    }

    fn raise(&mut self, region: u32, to: RegionCompleteness) {
        let mark = &mut self.regions[region as usize].completeness;
        *mark = (*mark).max(to);
    }

    /// A node's authored span touching recovery evidence makes its region
    /// at best `Recovered`.
    fn mark(&mut self, span: Span, region: u32) {
        if self.errors.touches(span) {
            self.raise(region, RegionCompleteness::Recovered);
        }
    }

    /// Settle the final markings and table indices, then store everything.
    fn freeze(mut self) -> Result<VueTemplateTopology<'p>, TopologyBuildError> {
        // Regions are created after their parents, so one reverse pass makes
        // every region at least as incomplete as the regions inside it.
        for index in (0..self.regions.len()).rev() {
            let parent = self.regions[index].parent;
            if parent != NONE {
                let mark = self.regions[index].completeness;
                let slot = &mut self.regions[parent as usize].completeness;
                *slot = (*slot).max(mark);
            }
        }
        let completeness = self
            .regions
            .iter()
            .map(|r| r.completeness)
            .max()
            .unwrap_or(RegionCompleteness::Complete);

        self.slot_rows.sort_by_key(|row| (row.component, row.start));
        // The index of each region in its kind's own table.
        let mut detail = vec![NONE; self.regions.len()];
        for row in &self.branch_rows {
            detail[row.region as usize] = row.chain;
        }
        for (index, &(region, _)) in self.for_rows.iter().enumerate() {
            detail[region as usize] = index as u32;
        }
        for (index, row) in self.slot_rows.iter().enumerate() {
            detail[row.region as usize] = index as u32;
        }

        let mut tree: Tree<'p> = Structure::new(self.source);
        let mut region_ids = Vec::with_capacity(self.regions.len());
        let mut node_ids = Vec::with_capacity(self.nodes.len());
        let region_data = |index: usize, regions: &[RegionRecord]| VueRegion {
            kind: regions[index].kind,
            completeness: regions[index].completeness,
            detail: detail[index],
        };
        region_ids.push(tree.add_region(None, None, region_data(0, &self.regions))?);
        let mut next_region = 1;
        for (index, node) in self.nodes.iter().enumerate() {
            let parent = (node.parent != NONE).then(|| node_ids[node.parent as usize]);
            let id = tree.push(
                parent,
                region_ids[node.region as usize],
                node.span,
                node.data,
            )?;
            node_ids.push(id);
            // Regions owned by this node were recorded after it and before
            // its first child.
            while self
                .regions
                .get(next_region)
                .is_some_and(|r| r.owner == index as u32)
            {
                let parent = region_ids[self.regions[next_region].parent as usize];
                region_ids.push(tree.add_region(
                    Some(parent),
                    Some(id),
                    region_data(next_region, &self.regions),
                )?);
                next_region += 1;
            }
        }
        if next_region != self.regions.len() {
            return Err(TopologyBuildError::InconsistentSyntaxTree);
        }

        // Rows of one chain are already in source order; grouping must keep
        // that order.
        self.branch_rows.sort_by_key(|row| row.chain);
        let mut if_chains: Arena<VueIfChainKey, VueIfChain, _> = Arena::new();
        let mut if_branches: FlatArena<VueIfBranchKey, VueIfBranch, _> = FlatArena::new();
        let mut chain_ids: Vec<VueIfChainId> = Vec::with_capacity(self.chain_owners.len());
        let mut rows = self.branch_rows.iter().peekable();
        for (number, &owner) in self.chain_owners.iter().enumerate() {
            let branches = if_branches.append(std::iter::from_fn(|| {
                rows.next_if(|row| row.chain as usize == number)
                    .map(|row| VueIfBranch {
                        region: region_ids[row.region as usize],
                        node: node_ids[row.node as usize],
                        role: row.role,
                    })
            }))?;
            let owner = node_ids[owner as usize];
            chain_ids.push(if_chains.push(VueIfChain { owner, branches })?);
        }
        let chain_of = self
            .chain_of_rows
            .iter()
            .map(|&(node, chain, index)| {
                (node_ids[node as usize], chain_ids[chain as usize], index)
            })
            .collect();
        let for_regions = self
            .for_rows
            .iter()
            .map(|&(region, node)| VueForRegion {
                region: region_ids[region as usize],
                node: node_ids[node as usize],
            })
            .collect();
        let slot_regions = self
            .slot_rows
            .iter()
            .map(|row| VueSlotRegion {
                region: region_ids[row.region as usize],
                component: node_ids[row.component as usize],
                template: row.template.map(|t| node_ids[t as usize]),
                implicit_default: row.implicit_default,
            })
            .collect();
        let by_syntax = self
            .by_syntax
            .iter()
            .map(|&node| (node != NONE).then(|| node_ids[node as usize]))
            .collect();

        Ok(VueTemplateTopology {
            ast: self.ast,
            store: tree.finish(),
            by_syntax,
            root: node_ids[0],
            root_region: region_ids[0],
            if_chains,
            if_branches,
            for_regions,
            slot_regions,
            chain_of,
            completeness,
        })
    }
}

fn is_slot_template(ast: &TemplateAst, child: crate::types::NodeId) -> bool {
    matches!(
        ast.nodes.get(child.0).map(|n| &n.kind),
        Some(AstNodeKind::Element(e)) if e.tag_type == TagType::Template && e.v_slot.is_some()
    )
}

/// Whitespace and comments between slot templates are not slot content.
fn is_ignorable(kind: &AstNodeKind) -> bool {
    match kind {
        AstNodeKind::Text(text) => text.is_whitespace_only,
        AstNodeKind::Comment(_) => true,
        _ => false,
    }
}

/// An element that never closed and is not allowed to stay open. The
/// parser already treats void elements as self-closing.
fn is_unclosed(element: &ElementNode) -> bool {
    element.tag_close.is_none() && !element.is_self_closing
}

fn element_span(element: &ElementNode) -> Span {
    let start = element.tag_open.start;
    let end = match (&element.tag_close, &element.content) {
        (Some(close), _) => close.end,
        (None, Some(content)) => content.end.max(element.tag_open.end),
        (None, None) => element.tag_open.end,
    };
    Span::new(start, end.max(start))
}

fn node_span_start(kind: &AstNodeKind) -> u32 {
    match kind {
        AstNodeKind::Element(element) => element.tag_open.start,
        AstNodeKind::Text(text) => text.start,
        AstNodeKind::Comment(comment) => comment.start,
        AstNodeKind::Interpolation(interpolation) => interpolation.start,
    }
}
