//! Shared FlowReturn/ResolveCall return equation.
//!
//! The SCC topology remains owned by the tagged obligation runtime. This
//! module owns only the two-domain return algebra; relation obligations never
//! enter its lattice.

use rustc_hash::FxHashMap;

use super::dispatch_txn::{
    ReturnDomainMetadata, ReturnEquationFailure, ReturnEquationMember, ReturnObligationIdentity,
};
use super::ProjectSemanticDispatch;
use crate::semantic_query::{FlowReturnKey, SemanticNodeData, SemanticNodeId};

impl<'a> ProjectSemanticDispatch<'a> {
    /// Solve the least fixed point
    /// `T(i) = normalize(S(i) union union(T(j), j in holds(i)))`.
    ///
    /// The lattice bottom is an empty leaf set and is never passed to the
    /// semantic union normalizer (whose empty result is `never`). A target
    /// outside `members` must already have a stable completed or warm result.
    pub(crate) fn solve_return_equation(
        &self,
        members: &[ReturnEquationMember],
        flow_overrides: &FxHashMap<FlowReturnKey, SemanticNodeId>,
    ) -> Result<Vec<SemanticNodeId>, ReturnEquationFailure> {
        let index: FxHashMap<&ReturnObligationIdentity, usize> = members
            .iter()
            .enumerate()
            .map(|(position, member)| (&member.identity, position))
            .collect();
        if index.len() != members.len() {
            return Err(ReturnEquationFailure::UnresolvedOutsideHold);
        }
        if members
            .iter()
            .any(|member| match (&member.identity, &member.domain) {
                (
                    ReturnObligationIdentity::FlowReturn(_),
                    ReturnDomainMetadata::FlowReturn { can_fall_through },
                ) => {
                    let _ = can_fall_through;
                    false
                }
                (ReturnObligationIdentity::ResolveCall(_), ReturnDomainMetadata::ResolveCall) => {
                    false
                }
                _ => true,
            })
        {
            return Err(ReturnEquationFailure::UnresolvedOutsideHold);
        }

        let seeds = members
            .iter()
            .map(|member| self.return_equation_leaf_set(&member.concrete_seeds))
            .collect::<Vec<_>>();
        let mut current = seeds.clone();
        let mut outside: FxHashMap<
            ReturnObligationIdentity,
            (Vec<SemanticNodeId>, Vec<SemanticNodeId>),
        > = FxHashMap::default();
        for member in members {
            for hold in &member.holds {
                if index.contains_key(hold) || outside.contains_key(hold) {
                    continue;
                }
                let Some((result, fresh)) = self
                    .stable_return_equation_target_override(hold, flow_overrides)
                    .or_else(|| self.stable_return_equation_target(hold))
                else {
                    return Err(ReturnEquationFailure::UnresolvedOutsideHold);
                };
                outside.insert(
                    hold.clone(),
                    (self.return_equation_leaf_set(&[result]), fresh),
                );
            }
        }

        loop {
            let mut progressed = false;
            let previous = current.clone();
            for (position, member) in members.iter().enumerate() {
                // Freshness is provenance consumed by position: a FLOW
                // member's return position widens a FRESH primitive-literal
                // leaf a held call contributed; a call member consuming
                // another call keeps the literal (a value position).
                let widen_fresh = matches!(member.domain, ReturnDomainMetadata::FlowReturn { .. });
                let mut leaves = seeds[position].clone();
                for hold in &member.holds {
                    let (contributed, fresh): (&[SemanticNodeId], &[SemanticNodeId]) =
                        if let Some(target) = index.get(hold) {
                            (&previous[*target], &members[*target].fresh_literal_returns)
                        } else if let Some((target, fresh)) = outside.get(hold) {
                            (target, fresh)
                        } else {
                            (&[], &[])
                        };
                    if widen_fresh && !fresh.is_empty() {
                        leaves.extend(contributed.iter().map(|leaf| {
                            if fresh.contains(leaf) {
                                self.widen_fresh_return_leaf(*leaf)
                            } else {
                                *leaf
                            }
                        }));
                    } else {
                        leaves.extend_from_slice(contributed);
                    }
                }
                let next = self.return_equation_leaf_set(&leaves);
                if next != current[position] {
                    current[position] = next;
                    progressed = true;
                }
            }
            if !progressed {
                break;
            }
        }

        if current.iter().any(Vec::is_empty) {
            return Err(ReturnEquationFailure::EmptyCycle);
        }
        Ok(current
            .iter()
            .map(|leaves| self.intern_normalized_union_or_intersection(leaves, true))
            .collect())
    }

    fn return_equation_leaf_set(&self, nodes: &[SemanticNodeId]) -> Vec<SemanticNodeId> {
        if nodes.is_empty() {
            return Vec::new();
        }
        let graph = self.graph();
        let mut leaves = Vec::new();
        for node in nodes {
            match graph.node_data(*node).as_deref() {
                Some(SemanticNodeData::Union(members)) => leaves.extend_from_slice(members),
                _ => leaves.push(*node),
            }
        }
        leaves.sort_by_key(|node| node.0);
        leaves.dedup();
        let normalized = self.intern_normalized_union_or_intersection(&leaves, true);
        match graph.node_data(normalized).as_deref() {
            Some(SemanticNodeData::Union(members)) => members.to_vec(),
            _ => vec![normalized],
        }
    }

    /// The just-discharged value of an in-component flow target — final
    /// at the close but not yet published, so it must never be read from
    /// the store.
    fn stable_return_equation_target_override(
        &self,
        identity: &ReturnObligationIdentity,
        flow_overrides: &FxHashMap<FlowReturnKey, SemanticNodeId>,
    ) -> Option<(SemanticNodeId, Vec<SemanticNodeId>)> {
        match identity {
            ReturnObligationIdentity::FlowReturn(key) => flow_overrides
                .get(key)
                .map(|return_type| (*return_type, Vec::new())),
            ReturnObligationIdentity::ResolveCall(_) => None,
        }
    }

    /// The stable value of a hold target outside the solving component,
    /// plus its FRESH primitive-literal returns (a resolved call whose
    /// result closed fresh; a flow target's own position already widened).
    fn stable_return_equation_target(
        &self,
        identity: &ReturnObligationIdentity,
    ) -> Option<(SemanticNodeId, Vec<SemanticNodeId>)> {
        match identity {
            ReturnObligationIdentity::FlowReturn(key) => {
                if let Some(result) = self
                    .dispatch_txn
                    .borrow()
                    .flow
                    .closed_values
                    .iter()
                    .find(|(member_key, _)| member_key == key)
                    .map(|(_, result)| result.return_type())
                {
                    return Some((result, Vec::new()));
                }
                self.graph()
                    .get_flow_return_result(self.ctx, key)
                    .map(|result| (result.return_type(), Vec::new()))
            }
            ReturnObligationIdentity::ResolveCall(key) => {
                if let Some(result) = self
                    .dispatch_txn
                    .borrow()
                    .call
                    .completed_members
                    .iter()
                    .find(|member| &member.key == key)
                    .map(|member| resolved_call_fresh_target(member.result.get()))
                {
                    return Some(result);
                }
                self.graph()
                    .get_resolve_call_result(self.ctx, key)
                    .as_ref()
                    .map(resolved_call_fresh_target)
            }
        }
    }

    /// Widen one FRESH primitive-literal leaf to its base primitive; every
    /// other leaf passes through unchanged.
    pub(super) fn widen_fresh_return_leaf(&self, leaf: SemanticNodeId) -> SemanticNodeId {
        let graph = self.graph();
        let primitive = match graph.node_data(leaf).as_deref() {
            Some(SemanticNodeData::Literal(value)) => match value {
                crate::semantic_query::LiteralValue::String(_) => {
                    crate::semantic_query::PrimitiveKind::String
                }
                crate::semantic_query::LiteralValue::Number(_) => {
                    crate::semantic_query::PrimitiveKind::Number
                }
                crate::semantic_query::LiteralValue::Boolean(_) => {
                    crate::semantic_query::PrimitiveKind::Boolean
                }
                crate::semantic_query::LiteralValue::BigInt(_) => {
                    crate::semantic_query::PrimitiveKind::BigInt
                }
            },
            _ => return leaf,
        };
        graph.intern_node(SemanticNodeData::Primitive(primitive))
    }
}

/// The return node of a resolved call plus its fresh-literal set (the
/// closed result's return when it closed fresh).
fn resolved_call_fresh_target(
    result: &crate::semantic_query::ResolvedCallResult,
) -> (SemanticNodeId, Vec<SemanticNodeId>) {
    match result {
        crate::semantic_query::ResolvedCallResult::Selected {
            return_type,
            fresh_literal_return,
            ..
        } => (
            *return_type,
            if *fresh_literal_return {
                vec![*return_type]
            } else {
                Vec::new()
            },
        ),
        crate::semantic_query::ResolvedCallResult::UnionSelected { return_type, .. }
        | crate::semantic_query::ResolvedCallResult::DynamicAny { return_type } => {
            (*return_type, Vec::new())
        }
    }
}

pub(super) fn resolved_call_return_type(
    result: &crate::semantic_query::ResolvedCallResult,
) -> SemanticNodeId {
    match result {
        crate::semantic_query::ResolvedCallResult::Selected { return_type, .. }
        | crate::semantic_query::ResolvedCallResult::UnionSelected { return_type, .. }
        | crate::semantic_query::ResolvedCallResult::DynamicAny { return_type } => *return_type,
    }
}
