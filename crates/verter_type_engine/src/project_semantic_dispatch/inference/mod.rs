//! The inference owner: the candidates a collecting session fixes from,
//! and the type each inference variable fixes to (`getInferredType`) — for
//! a conditional's `infer` declarations and a reverse mapped type
//! (`getTypeFromInference`), and for a signature's type parameters.

mod collect;
pub mod constraints;
pub mod fixation;
pub mod session;

pub use fixation::{winning_candidates, WinningCandidates};

use super::dispatch_txn::{RelationStep, StrictFamilyConfig};
use super::ProjectSemanticDispatch;
use crate::semantic_query::{
    NodeScopeId, ProjectionReductionContext, RelationKind, SemanticNodeId, VariancePhase,
};
use std::sync::Arc;
/// Fixed inference demands enter the same concrete query driver.
pub(in crate::project_semantic_dispatch) trait InferenceDemandDriver {
    fn call_relation(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
        freshness_origin: SemanticNodeId,
        budget: &mut super::call_resolve::CallResolutionBudget<'_>,
        binding_enabled: bool,
        excess_property_check: bool,
    ) -> RelationStep;
    fn execute_relate_pair(&self, source: SemanticNodeId, target: SemanticNodeId) -> RelationStep;
    fn execute_relate_pair_kind(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
        relation: RelationKind,
    ) -> RelationStep;
    fn freshness_for_source_node(
        &self,
        node: SemanticNodeId,
    ) -> crate::semantic_query::FreshnessKey;
    fn intern_normalized_union_or_intersection(
        &self,
        members: &[SemanticNodeId],
        is_union: bool,
    ) -> SemanticNodeId;
    fn locator_binder_frame_from_narrow_params(
        &self,
        scope: &NodeScopeId,
        owner_symbol: &Arc<str>,
        type_parameters: &[verter_type_expr::facts::NarrowTypeParam],
    ) -> Vec<(Arc<str>, SemanticNodeId)>;
    fn normalize_node_for_structural_fact_demand(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
    ) -> super::evaluate::StructuralFactDemandOutcome;
    fn relation_combine_candidates(
        &self,
        nodes: &[SemanticNodeId],
        variance: VariancePhase,
    ) -> SemanticNodeId;
    fn relation_strict_config(&self) -> StrictFamilyConfig;
    fn substitute_bindings(
        &self,
        node: SemanticNodeId,
        bindings: &[crate::semantic_query::InferBinding],
    ) -> SemanticNodeId;
    fn substitute_semantic_type_param(
        &self,
        node: SemanticNodeId,
        parameter_node: SemanticNodeId,
        arg: SemanticNodeId,
    ) -> SemanticNodeId;
    fn nodes_provably_equal(&self, a: SemanticNodeId, b: SemanticNodeId) -> bool;
}
impl<C: crate::resolver_core::ResolverCapabilities> InferenceDemandDriver
    for ProjectSemanticDispatch<'_, C>
{
    fn call_relation(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
        freshness_origin: SemanticNodeId,
        budget: &mut super::call_resolve::CallResolutionBudget<'_>,
        binding_enabled: bool,
        excess_property_check: bool,
    ) -> RelationStep {
        ProjectSemanticDispatch::call_relation(
            self,
            source,
            target,
            freshness_origin,
            budget,
            binding_enabled,
            excess_property_check,
        )
    }
    fn execute_relate_pair(&self, source: SemanticNodeId, target: SemanticNodeId) -> RelationStep {
        ProjectSemanticDispatch::execute_relate_pair(self, source, target)
    }
    fn execute_relate_pair_kind(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
        relation: RelationKind,
    ) -> RelationStep {
        ProjectSemanticDispatch::execute_relate_pair_kind(self, source, target, relation)
    }
    fn freshness_for_source_node(
        &self,
        node: SemanticNodeId,
    ) -> crate::semantic_query::FreshnessKey {
        ProjectSemanticDispatch::freshness_for_source_node(self, node)
    }
    fn intern_normalized_union_or_intersection(
        &self,
        members: &[SemanticNodeId],
        is_union: bool,
    ) -> SemanticNodeId {
        ProjectSemanticDispatch::intern_normalized_union_or_intersection(self, members, is_union)
    }
    fn locator_binder_frame_from_narrow_params(
        &self,
        scope: &NodeScopeId,
        owner_symbol: &Arc<str>,
        type_parameters: &[verter_type_expr::facts::NarrowTypeParam],
    ) -> Vec<(Arc<str>, SemanticNodeId)> {
        ProjectSemanticDispatch::locator_binder_frame_from_narrow_params(
            self,
            scope,
            owner_symbol,
            type_parameters,
        )
    }
    fn normalize_node_for_structural_fact_demand(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
    ) -> super::evaluate::StructuralFactDemandOutcome {
        ProjectSemanticDispatch::normalize_node_for_structural_fact_demand(self, node, context)
    }
    fn relation_combine_candidates(
        &self,
        nodes: &[SemanticNodeId],
        variance: VariancePhase,
    ) -> SemanticNodeId {
        ProjectSemanticDispatch::relation_combine_candidates(self, nodes, variance)
    }
    fn relation_strict_config(&self) -> StrictFamilyConfig {
        ProjectSemanticDispatch::relation_strict_config(self)
    }
    fn substitute_bindings(
        &self,
        node: SemanticNodeId,
        bindings: &[crate::semantic_query::InferBinding],
    ) -> SemanticNodeId {
        ProjectSemanticDispatch::substitute_bindings(self, node, bindings)
    }
    fn substitute_semantic_type_param(
        &self,
        node: SemanticNodeId,
        parameter_node: SemanticNodeId,
        arg: SemanticNodeId,
    ) -> SemanticNodeId {
        ProjectSemanticDispatch::substitute_semantic_type_param(self, node, parameter_node, arg)
    }
    fn nodes_provably_equal(&self, a: SemanticNodeId, b: SemanticNodeId) -> bool {
        crate::semantic_query::stable_key::provably_equal(self.graph(), a, b)
    }
}
/// Selected arena/source authority; no whole transaction, query lease, or
/// ambient resolver can be obtained through this view.
pub(in crate::project_semantic_dispatch) struct InferenceTxn<'a, D: InferenceDemandDriver> {
    driver: &'a D,
    arena: super::arena_ops::ArenaOps<'a>,
    source: &'a dyn crate::resolver_core::request_ports::OwnedLowering,
}
impl<'b, 'a, C: crate::resolver_core::ResolverCapabilities>
    InferenceTxn<'b, ProjectSemanticDispatch<'a, C>>
{
    fn new(driver: &'b ProjectSemanticDispatch<'a, C>) -> Self {
        Self {
            driver,
            arena: super::arena_ops::ArenaOps::new(driver.graph()),
            source: driver.ctx,
        }
    }
}
impl<D: InferenceDemandDriver> InferenceTxn<'_, D> {
    fn graph(&self) -> &super::arena_ops::ArenaOps<'_> {
        &self.arena
    }
    pub(in crate::project_semantic_dispatch) fn call_relation(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
        freshness_origin: SemanticNodeId,
        budget: &mut super::call_resolve::CallResolutionBudget<'_>,
        binding_enabled: bool,
        excess_property_check: bool,
    ) -> RelationStep {
        self.driver.call_relation(
            source,
            target,
            freshness_origin,
            budget,
            binding_enabled,
            excess_property_check,
        )
    }
    pub(in crate::project_semantic_dispatch) fn execute_relate_pair(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
    ) -> RelationStep {
        self.driver.execute_relate_pair(source, target)
    }
    pub(in crate::project_semantic_dispatch) fn execute_relate_pair_kind(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
        relation: RelationKind,
    ) -> RelationStep {
        self.driver
            .execute_relate_pair_kind(source, target, relation)
    }
    pub(in crate::project_semantic_dispatch) fn freshness_for_source_node(
        &self,
        node: SemanticNodeId,
    ) -> crate::semantic_query::FreshnessKey {
        self.driver.freshness_for_source_node(node)
    }
    pub(in crate::project_semantic_dispatch) fn intern_normalized_union_or_intersection(
        &self,
        members: &[SemanticNodeId],
        is_union: bool,
    ) -> SemanticNodeId {
        self.driver
            .intern_normalized_union_or_intersection(members, is_union)
    }
    pub(in crate::project_semantic_dispatch) fn locator_binder_frame_from_narrow_params(
        &self,
        scope: &NodeScopeId,
        owner_symbol: &Arc<str>,
        type_parameters: &[verter_type_expr::facts::NarrowTypeParam],
    ) -> Vec<(Arc<str>, SemanticNodeId)> {
        self.driver
            .locator_binder_frame_from_narrow_params(scope, owner_symbol, type_parameters)
    }
    pub(in crate::project_semantic_dispatch) fn normalize_node_for_structural_fact_demand(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
    ) -> super::evaluate::StructuralFactDemandOutcome {
        self.driver
            .normalize_node_for_structural_fact_demand(node, context)
    }
    pub(in crate::project_semantic_dispatch) fn relation_combine_candidates(
        &self,
        nodes: &[SemanticNodeId],
        variance: VariancePhase,
    ) -> SemanticNodeId {
        self.driver.relation_combine_candidates(nodes, variance)
    }
    pub(in crate::project_semantic_dispatch) fn relation_strict_config(
        &self,
    ) -> StrictFamilyConfig {
        self.driver.relation_strict_config()
    }
    pub(in crate::project_semantic_dispatch) fn substitute_bindings(
        &self,
        node: SemanticNodeId,
        bindings: &[crate::semantic_query::InferBinding],
    ) -> SemanticNodeId {
        self.driver.substitute_bindings(node, bindings)
    }
    pub(in crate::project_semantic_dispatch) fn substitute_semantic_type_param(
        &self,
        node: SemanticNodeId,
        parameter_node: SemanticNodeId,
        arg: SemanticNodeId,
    ) -> SemanticNodeId {
        self.driver
            .substitute_semantic_type_param(node, parameter_node, arg)
    }
    fn nodes_provably_equal(&self, a: SemanticNodeId, b: SemanticNodeId) -> bool {
        self.driver.nodes_provably_equal(a, b)
    }
}
