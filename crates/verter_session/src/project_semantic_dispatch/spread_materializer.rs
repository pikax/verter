//! Ordered object-literal spread lowering.
//!
//! Known members are folded onto one object surface. A spread operand whose
//! surface cannot be enumerated is retained inside that Object's
//! [`MemberSurfaceCompleteness::OpenSpread`] metadata by node identity.
//! No spread path creates a source-side intersection.

use std::sync::Arc;

use rustc_hash::FxHashMap;
use verter_type_expr::{ExcessPropertyOrigin, ObjectExpr, ObjectMember, TypeExpr};

use super::ProjectSemanticDispatch;
use crate::resolver_core::bare_name_resolve::DeclarationScopePayload;
use crate::resolver_core::scope_shadowing::ScopeShadowing;
use crate::semantic_query::{
    IndexSignature, MemberSurfaceCompleteness, NodeScopeId, OpenSpreadOperands, PrimitiveKind,
    ProjectionReductionContext, QueryResult, RelationResult, SemanticNodeData, SemanticNodeId,
    SemanticQueryKey, SurfaceMember, SurfaceView,
};
use verter_semantic::analysis::type_solver::host::ResolvedRootIdentity;

/// Maximum number of concrete alternatives produced by one spread fold.
pub(crate) const SPREAD_UNION_DISTRIBUTION_CAP: usize = 1024;

/// Origin policy for one ordered fold segment.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum FoldSegmentKind {
    DirectRun,
    SpreadOperand,
}

#[derive(Clone)]
struct SpreadFoldState {
    view: SurfaceView,
    open_operands: Vec<SemanticNodeId>,
    left_is_initial: bool,
}

impl SpreadFoldState {
    fn new() -> Self {
        Self {
            view: empty_surface_view(),
            open_operands: Vec::new(),
            left_is_initial: true,
        }
    }

    fn finish(mut self) -> Option<SurfaceView> {
        let completeness = if self.open_operands.is_empty() {
            MemberSurfaceCompleteness::Closed
        } else {
            MemberSurfaceCompleteness::OpenSpread(OpenSpreadOperands::new(Arc::from(
                self.open_operands.into_boxed_slice(),
            )))
        };
        self.view.replace_completeness(completeness);
        Some(self.view)
    }
}

impl<'a> ProjectSemanticDispatch<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn lower_spread_object_literal(
        &self,
        obj: &ObjectExpr,
        env: &FxHashMap<String, SemanticNodeId>,
        scope: &NodeScopeId,
        name_resolution: &FxHashMap<Arc<str>, ResolvedRootIdentity>,
        scope_payload: Option<&DeclarationScopePayload>,
        shadowing: &ScopeShadowing,
        substitutions: &mut Vec<(Arc<str>, SemanticNodeId)>,
        reduction_context: ProjectionReductionContext,
    ) -> SemanticNodeId {
        let mut segments: Vec<(FoldSegmentKind, SemanticNodeId)> = Vec::new();
        let mut run: Vec<ObjectMember> = Vec::new();
        let lower_run =
            |run: &mut Vec<ObjectMember>,
             substitutions: &mut Vec<(Arc<str>, SemanticNodeId)>,
             segments: &mut Vec<(FoldSegmentKind, SemanticNodeId)>| {
                if run.is_empty() {
                    return;
                }
                let run_obj = TypeExpr::Object(Arc::new(ObjectExpr {
                    properties: std::mem::take(run),
                }));
                let node = self.shallow_lower_type_expr_with_context(
                    &run_obj,
                    env,
                    scope,
                    name_resolution,
                    scope_payload,
                    shadowing,
                    substitutions,
                    reduction_context,
                );
                segments.push((FoldSegmentKind::DirectRun, node));
            };

        for member in &obj.properties {
            match member {
                ObjectMember::Spread(spread) => {
                    lower_run(&mut run, substitutions, &mut segments);
                    let operand = self.shallow_lower_type_expr_with_context(
                        &spread.ty,
                        env,
                        scope,
                        name_resolution,
                        scope_payload,
                        shadowing,
                        substitutions,
                        reduction_context.into_structural_provenance(),
                    );
                    segments.push((
                        FoldSegmentKind::SpreadOperand,
                        self.taint_spread_node(operand),
                    ));
                }
                other => run.push(other.clone()),
            }
        }
        lower_run(&mut run, substitutions, &mut segments);
        self.fold_spread_segments(segments, scope)
    }

    /// Fold ordered direct/spread segments. Concrete union arms distribute
    /// while the bounded product remains representable. Exhaustion retains
    /// the union node as one open operand.
    pub(crate) fn fold_spread_segments(
        &self,
        segments: Vec<(FoldSegmentKind, SemanticNodeId)>,
        scope: &NodeScopeId,
    ) -> SemanticNodeId {
        let mut states = vec![SpreadFoldState::new()];
        for (kind, segment) in segments {
            states = self.fold_segment_alternatives(states, segment, kind, 0);
        }

        let graph = self.graph();
        let mut nodes: Vec<SemanticNodeId> = Vec::with_capacity(states.len());
        for state in states {
            if let Some(view) = state.finish() {
                nodes.push(
                    graph.intern_node_with_scope(SemanticNodeData::Object(view), scope.clone()),
                );
            }
        }
        nodes.sort_unstable();
        nodes.dedup();
        if nodes.len() == 1 {
            return nodes[0];
        }
        let members = Arc::from(nodes.clone().into_boxed_slice());
        match self
            .execute_read(SemanticQueryKey::NormalizeUnion {
                members: Arc::clone(&members),
            })
            .value
        {
            QueryResult::Value(id) => id,
            _ => graph.intern_node_with_scope(SemanticNodeData::Union(members), scope.clone()),
        }
    }

    fn fold_segment_alternatives(
        &self,
        states: Vec<SpreadFoldState>,
        segment: SemanticNodeId,
        kind: FoldSegmentKind,
        depth: usize,
    ) -> Vec<SpreadFoldState> {
        let graph = self.graph();
        let Some(data) = graph.node_data(segment) else {
            return states
                .into_iter()
                .map(|mut state| {
                    self.retain_open_spread_operand(&mut state, segment);
                    state.left_is_initial = false;
                    state
                })
                .collect();
        };
        let SemanticNodeData::Union(arms) = data.as_ref() else {
            drop(data);
            return states
                .into_iter()
                .map(|mut state| {
                    if self.fold_one_segment(&mut state, segment, kind) {
                        state.left_is_initial = false;
                    }
                    state
                })
                .collect();
        };
        let arms = Arc::clone(arms);
        drop(data);
        let product = states.len().saturating_mul(arms.len());
        if depth >= 4
            || arms.len() > SPREAD_UNION_DISTRIBUTION_CAP
            || product > SPREAD_UNION_DISTRIBUTION_CAP
        {
            return states
                .into_iter()
                .map(|mut state| {
                    self.retain_open_spread_operand(&mut state, segment);
                    state.left_is_initial = false;
                    state
                })
                .collect();
        }

        let mut distributed = Vec::with_capacity(product);
        for state in states {
            for arm in arms.iter() {
                let branch =
                    self.fold_segment_alternatives(vec![state.clone()], *arm, kind, depth + 1);
                distributed.extend(branch);
            }
        }
        distributed
    }

    fn fold_one_segment(
        &self,
        state: &mut SpreadFoldState,
        segment: SemanticNodeId,
        kind: FoldSegmentKind,
    ) -> bool {
        let graph = self.graph();
        let Some(data) = graph.node_data(segment) else {
            self.retain_open_spread_operand(state, segment);
            return true;
        };
        match data.as_ref() {
            SemanticNodeData::Primitive(PrimitiveKind::Any | PrimitiveKind::Unknown) => {
                drop(data);
                self.retain_open_spread_operand(state, segment);
                true
            }
            SemanticNodeData::Primitive(
                PrimitiveKind::Never | PrimitiveKind::Null | PrimitiveKind::Undefined,
            ) => false,
            data if right_spreads_to_nothing(data) => false,
            SemanticNodeData::Object(view) => {
                let view = view.clone();
                drop(data);
                self.fold_surface_segment(state, &view, kind);
                true
            }
            _ => {
                drop(data);
                self.retain_open_spread_operand(state, segment);
                true
            }
        }
    }

    fn fold_surface_segment(
        &self,
        state: &mut SpreadFoldState,
        right: &SurfaceView,
        kind: FoldSegmentKind,
    ) {
        if let Some(open) = right.open_spread_operands() {
            self.taint_members_before_open_spread(state);
            state.open_operands.extend(open.as_slice().iter().copied());
        }
        self.merge_concrete_surface(state, right, kind);
    }

    fn retain_open_spread_operand(&self, state: &mut SpreadFoldState, operand: SemanticNodeId) {
        self.taint_members_before_open_spread(state);
        state.open_operands.push(operand);
    }

    fn taint_members_before_open_spread(&self, state: &mut SpreadFoldState) {
        let unknown = self
            .graph()
            .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Unknown));
        let members: Vec<SurfaceMember> = state
            .view
            .positive_members()
            .iter()
            .map(|member| {
                let mut member = member.clone();
                member.value = unknown;
                member.is_method = false;
                member.readonly = false;
                member.spans = Default::default();
                member.declaration_origin = None;
                member.declared_in_macro_type_arg =
                    crate::semantic_query::MacroOwnBodyStamp::NEUTRAL;
                member.merge_role = crate::semantic_query::MergeRoleStamp::NEUTRAL;
                member.excess_origin = ExcessPropertyOrigin::SpreadTainted;
                member
            })
            .collect();
        state.view = crate::semantic_query::surface_view! {
            members: Arc::from(members.into_boxed_slice()),
            call_signatures: Arc::clone(&state.view.call_signatures),
            construct_signatures: Arc::clone(&state.view.construct_signatures),
            index_signatures: Arc::from([]),
            keyspace: None,
            has_index_signature: false,
            completeness: MemberSurfaceCompleteness::Closed,
        };
    }

    fn merge_concrete_surface(
        &self,
        state: &mut SpreadFoldState,
        right: &SurfaceView,
        kind: FoldSegmentKind,
    ) {
        let merged = self.merge_spread_surfaces(&state.view, right, kind, state.left_is_initial);
        state.view = merged;
    }

    /// Spread-copy a node's visible members. Non-surface operands remain
    /// untouched so the fold can retain their exact typed identity.
    pub(crate) fn taint_spread_node(&self, node: SemanticNodeId) -> SemanticNodeId {
        let graph = self.graph();
        let Some(data) = graph.node_data(node) else {
            return node;
        };
        match data.as_ref() {
            SemanticNodeData::Object(view) => {
                let tainted = taint_surface_view(view);
                drop(data);
                graph.intern_preserving_scope(node, SemanticNodeData::Object(tainted))
            }
            SemanticNodeData::Union(arms) => {
                let arms = Arc::clone(arms);
                drop(data);
                let tainted: Vec<SemanticNodeId> = arms
                    .iter()
                    .map(|arm| self.taint_spread_node(*arm))
                    .collect();
                if tainted.as_slice() == arms.as_ref() {
                    node
                } else {
                    graph.intern_preserving_scope(
                        node,
                        SemanticNodeData::Union(Arc::from(tainted.into_boxed_slice())),
                    )
                }
            }
            _ => node,
        }
    }

    fn merge_spread_surfaces(
        &self,
        left: &SurfaceView,
        right: &SurfaceView,
        kind: FoldSegmentKind,
        left_is_initial: bool,
    ) -> SurfaceView {
        let skipped: Vec<&Arc<str>> = right
            .positive_members()
            .iter()
            .filter(|member| !member.visibility.is_public())
            .map(|member| &member.name)
            .collect();
        let mut members: Vec<SurfaceMember> = Vec::with_capacity(left.positive_members().len());
        let mut consumed_right = vec![false; right.positive_members().len()];
        for left_member in left.positive_members().iter() {
            if skipped.iter().any(|name| **name == left_member.name) {
                continue;
            }
            let overlap = right.positive_members().iter().position(|right_member| {
                right_member.name == left_member.name && right_member.visibility.is_public()
            });
            let Some(right_index) = overlap else {
                members.push(left_member.clone());
                continue;
            };
            consumed_right[right_index] = true;
            let right_member = &right.positive_members()[right_index];
            match kind {
                FoldSegmentKind::DirectRun => members.push(right_member.clone()),
                FoldSegmentKind::SpreadOperand if !right_member.optional => {
                    members.push(right_member.clone())
                }
                FoldSegmentKind::SpreadOperand => {
                    let value = if left_member.value == right_member.value {
                        left_member.value
                    } else {
                        self.subtype_reduced_union(left_member.value, right_member.value)
                    };
                    let mut merged = left_member.clone();
                    merged.value = value;
                    merged.is_method = false;
                    merged.readonly = false;
                    merged.excess_origin = ExcessPropertyOrigin::SpreadTainted;
                    members.push(merged);
                }
            }
        }
        for (index, right_member) in right.positive_members().iter().enumerate() {
            if consumed_right[index] || !right_member.visibility.is_public() {
                continue;
            }
            if !members
                .iter()
                .any(|member| member.name == right_member.name)
            {
                members.push(right_member.clone());
            }
        }

        let index_signatures: Vec<IndexSignature> = if left_is_initial {
            right.index_signatures.to_vec()
        } else {
            left.index_signatures
                .iter()
                .filter_map(|left_index| {
                    let right_index = right
                        .index_signatures
                        .iter()
                        .find(|candidate| candidate.key_type == left_index.key_type)?;
                    Some(IndexSignature {
                        key_type: left_index.key_type,
                        value_type: if left_index.value_type == right_index.value_type {
                            left_index.value_type
                        } else {
                            self.subtype_reduced_union(
                                left_index.value_type,
                                right_index.value_type,
                            )
                        },
                        readonly: left_index.readonly || right_index.readonly,
                        spans: left_index.spans,
                        declaration_origin: left_index.declaration_origin.clone(),
                    })
                })
                .collect()
        };
        let has_index_signature = !index_signatures.is_empty();
        crate::semantic_query::surface_view! {
            members: Arc::from(members.into_boxed_slice()),
            call_signatures: Arc::from([]),
            construct_signatures: Arc::from([]),
            index_signatures: Arc::from(index_signatures.into_boxed_slice()),
            keyspace: None,
            has_index_signature,
            completeness: MemberSurfaceCompleteness::Closed,
        }
    }

    pub(crate) fn subtype_reduced_union(
        &self,
        left: SemanticNodeId,
        right: SemanticNodeId,
    ) -> SemanticNodeId {
        if left == right {
            return left;
        }
        let graph = self.graph();
        let mut arms: Vec<SemanticNodeId> = Vec::new();
        for side in [left, right] {
            match graph.node_data(side).as_deref() {
                Some(SemanticNodeData::Union(side_arms)) => {
                    arms.extend(side_arms.iter().copied());
                }
                _ => arms.push(side),
            }
        }
        let mut unique = Vec::with_capacity(arms.len());
        for arm in arms {
            if !unique.contains(&arm) {
                unique.push(arm);
            }
        }
        let mut arms = unique;
        const PAIRWISE_REDUCTION_ARM_CAP: usize = 16;
        if arms.len() <= PAIRWISE_REDUCTION_ARM_CAP {
            let assignable = |source: SemanticNodeId, target: SemanticNodeId| {
                matches!(
                    self.execute_relate_pair_result(source, target),
                    RelationResult::Assignable { .. }
                )
            };
            let mut kept = Vec::with_capacity(arms.len());
            for (index, arm) in arms.iter().enumerate() {
                let absorbed = arms.iter().enumerate().any(|(other_index, other)| {
                    index != other_index
                        && arm != other
                        && assignable(*arm, *other)
                        && (!assignable(*other, *arm) || other_index < index)
                });
                if !absorbed {
                    kept.push(*arm);
                }
            }
            arms = kept;
        }
        if arms.len() == 1 {
            return arms[0];
        }
        match self
            .execute_read(SemanticQueryKey::NormalizeUnion {
                members: Arc::from(arms.clone().into_boxed_slice()),
            })
            .value
        {
            QueryResult::Value(id) => id,
            _ => graph.intern_node(SemanticNodeData::Union(Arc::from(arms.into_boxed_slice()))),
        }
    }

    fn execute_relate_pair_result(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
    ) -> RelationResult {
        use super::relation_txn::RelationStep;
        match self.execute_relate_pair(source, target) {
            RelationStep::Assignable { bindings } => RelationResult::Assignable { bindings },
            RelationStep::NotAssignable => RelationResult::NotAssignable,
            RelationStep::Unknown | RelationStep::BudgetExceeded(_) | RelationStep::Assumed => {
                RelationResult::Unknown
            }
        }
    }
}

fn empty_surface_view() -> SurfaceView {
    crate::semantic_query::surface_view! {
        members: Arc::from([]),
        call_signatures: Arc::from([]),
        construct_signatures: Arc::from([]),
        index_signatures: Arc::from([]),
        keyspace: None,
        has_index_signature: false,
        completeness: MemberSurfaceCompleteness::Closed,
    }
}

fn right_spreads_to_nothing(data: &SemanticNodeData) -> bool {
    matches!(
        data,
        SemanticNodeData::Primitive(
            PrimitiveKind::Boolean
                | PrimitiveKind::Number
                | PrimitiveKind::BigInt
                | PrimitiveKind::String
                | PrimitiveKind::Object,
        ) | SemanticNodeData::Literal(_)
            | SemanticNodeData::TemplateLiteral { .. }
            | SemanticNodeData::Signature { .. }
    )
}

fn taint_surface_view(view: &SurfaceView) -> SurfaceView {
    let members: Vec<SurfaceMember> = view
        .positive_members()
        .iter()
        .map(|member| {
            let mut member = member.clone();
            if member.visibility.is_public() {
                member.is_method = false;
                member.readonly = false;
            }
            member.excess_origin = ExcessPropertyOrigin::SpreadTainted;
            member
        })
        .collect();
    view.clone()
        .with_positive_members(Arc::from(members.into_boxed_slice()))
}
