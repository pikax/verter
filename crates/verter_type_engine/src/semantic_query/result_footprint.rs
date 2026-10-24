//! Allocation footprints of the query results a cache retains whole: the
//! flow-return and call-resolution carriers with every key, substitution and
//! slice they keep reachable, and the relation, runtime-classification and
//! cycle-gate payloads.
//!
//! Declared under the flow-return result's module so its private fields can
//! be destructured. Every struct is destructured with all fields named and
//! every enum matched without a wildcard, so a new field or variant fails to
//! compile until its heap is accounted for. A field asserted with
//! [`copy_inline`] owns no heap: a `Copy` type cannot own an allocation.

use verter_session_query::retention::allocation::AliasSet;

use super::super::allocation_footprint::{copy_inline, OwnedHeap};
use super::super::{
    BroadRuntimeClassification, CanonicalTypeSubstitution, FlowFunctionSlotIdentity,
    FlowInputContext, FlowReturnContext, FlowReturnKey, MaterializationCycleGateFallbackReasons,
    MaterializationCycleGateOutcome, RecursionIdentities, RelationOutcome, RelationPayload,
    RelationRecursionFootprint, ResolvedCallResult, ResolvedDeclSlotIdentity,
    ReturnProjectionDemand, SignatureCandidateOrigin, SignatureOccurrenceIdentity,
};
#[cfg(test)]
use super::SemanticNodeId;
use super::{FlowReturnResult, FunctionKindFacet};
use crate::semantic_query::demand::{Demand, EvalPolicy, ProjectionDemand};

impl OwnedHeap for FlowReturnResult {
    /// The fresh-arm and diagnostic slices and both independent proofs.
    /// The return node is the arena's; a delivered result's owner pins its
    /// own metadata.
    fn owned_heap_bytes(&self, aliases: &mut AliasSet) -> usize {
        let Self {
            independent_member,
            observed_completeness,
            exact_return_keys,
            return_type: _,
            can_fall_through,
            degradation,
            fresh_literal_arms,
            facet,
            checker_diagnostics,
            owner: _,
        } = self;
        independent_member.owned_heap_bytes(aliases)
            + copy_inline(observed_completeness)
            + exact_return_keys.owned_heap_bytes(aliases)
            + copy_inline(can_fall_through)
            + copy_inline(degradation)
            + fresh_literal_arms.owned_heap_bytes(aliases)
            + function_kind_facet_bytes(facet)
            + checker_diagnostics.owned_heap_bytes(aliases)
    }
}

/// A function-kind facet holds node ordinals and flags only.
fn function_kind_facet_bytes(facet: &FunctionKindFacet) -> usize {
    match facet {
        FunctionKindFacet::None => 0,
        FunctionKindFacet::Wrap(wrap) => copy_inline(wrap),
        FunctionKindFacet::Predicate(predicate) => copy_inline(predicate),
    }
}

impl OwnedHeap for ResolvedCallResult {
    /// The winner's origin, its substitution and its fresh-return slice.
    /// Signature and return nodes are the arena's.
    fn owned_heap_bytes(&self, aliases: &mut AliasSet) -> usize {
        match self {
            ResolvedCallResult::Selected {
                selected,
                selected_signature: _,
                substitution,
                return_type: _,
                fresh_literal_returns,
                recovery_diagnostic,
            } => {
                selected.owned_heap_bytes(aliases)
                    + substitution.owned_heap_bytes(aliases)
                    + fresh_literal_returns.owned_heap_bytes(aliases)
                    + copy_inline(recovery_diagnostic)
            }
            ResolvedCallResult::DynamicAny { return_type: _ } => 0,
        }
    }
}

impl OwnedHeap for SignatureCandidateOrigin {
    fn owned_heap_bytes(&self, aliases: &mut AliasSet) -> usize {
        match self {
            SignatureCandidateOrigin::Authored(SignatureOccurrenceIdentity {
                function,
                signature_ordinal,
            }) => function.owned_heap_bytes(aliases) + copy_inline(signature_ordinal),
            SignatureCandidateOrigin::Rootless => 0,
        }
    }
}

impl OwnedHeap for CanonicalTypeSubstitution {
    /// The pair list's block and its root attachment's owner.
    fn owned_heap_bytes(&self, aliases: &mut AliasSet) -> usize {
        let Self(bindings, roots) = self;
        bindings.owned_heap_bytes(aliases) + roots.owned_heap_bytes(aliases)
    }
}

impl OwnedHeap for FlowReturnKey {
    fn owned_heap_bytes(&self, aliases: &mut AliasSet) -> usize {
        let Self {
            function,
            normalized_type_args,
            context,
            demand,
            input,
            result_contract,
        } = self;
        function.owned_heap_bytes(aliases)
            + normalized_type_args.owned_heap_bytes(aliases)
            + context.owned_heap_bytes(aliases)
            + demand.owned_heap_bytes(aliases)
            + input.owned_heap_bytes(aliases)
            // The canonical bytes: one `Arc` block every clone of the
            // contract shares, counted once per `aliases`.
            + verter_session_query::retention::allocation::shared_encoding_bytes(
                result_contract.canonical_shared_bytes(),
                aliases,
            )
    }
}

impl OwnedHeap for FlowFunctionSlotIdentity {
    fn owned_heap_bytes(&self, aliases: &mut AliasSet) -> usize {
        let Self {
            declaration_slot,
            function_part,
            overload_ordinal,
        } = self;
        declaration_slot.owned_heap_bytes(aliases)
            + function_part_bytes(function_part, aliases)
            + copy_inline(overload_ordinal)
    }
}

fn function_part_bytes(
    part: &verter_type_expr::facts::FunctionPartIdentity,
    aliases: &mut AliasSet,
) -> usize {
    use verter_type_expr::facts::FunctionPartIdentity;
    match part {
        FunctionPartIdentity::Member { member_path } => member_path.owned_heap_bytes(aliases),
        FunctionPartIdentity::DeclarationBody
        | FunctionPartIdentity::Initializer
        | FunctionPartIdentity::Other { ordinal: _ } => 0,
    }
}

impl OwnedHeap for ResolvedDeclSlotIdentity {
    fn owned_heap_bytes(&self, aliases: &mut AliasSet) -> usize {
        let Self {
            defining_canonical,
            owner,
            merged_symbol_name,
            symbol_space,
            env,
        } = self;
        defining_canonical.owned_heap_bytes(aliases)
            + copy_inline(owner)
            + merged_symbol_name.owned_heap_bytes(aliases)
            + copy_inline(symbol_space)
            + copy_inline(env)
    }
}

impl OwnedHeap for FlowReturnContext {
    fn owned_heap_bytes(&self, aliases: &mut AliasSet) -> usize {
        let Self {
            parse_env_hash,
            resolve_env_hash,
            type_env_hash,
            lib_env_hash,
            project_identity,
            result_evaluation,
            type_substitution,
            policy,
        } = self;
        copy_inline(parse_env_hash)
            + copy_inline(resolve_env_hash)
            + copy_inline(type_env_hash)
            + copy_inline(lib_env_hash)
            + copy_inline(project_identity)
            + copy_inline(result_evaluation)
            + type_substitution.owned_heap_bytes(aliases)
            + copy_inline(policy)
    }
}

impl OwnedHeap for ReturnProjectionDemand {
    fn owned_heap_bytes(&self, aliases: &mut AliasSet) -> usize {
        let Self { point } = self;
        let Demand { projection, policy } = point;
        let ProjectionDemand {
            path,
            facets,
            member_demand,
            call_signatures,
            construct_signatures,
            index_signatures,
            display_needs,
        } = projection;
        let EvalPolicy {
            alias_preservation,
            normalization_depth,
            generic_open,
            operator_reduction,
            carrier_stop,
            surface_role,
            provenance,
            merge_role,
        } = policy;
        path.owned_heap_bytes(aliases)
            + copy_inline(facets)
            + copy_inline(member_demand)
            + copy_inline(call_signatures)
            + copy_inline(construct_signatures)
            + copy_inline(index_signatures)
            + copy_inline(display_needs)
            + copy_inline(alias_preservation)
            + copy_inline(normalization_depth)
            + copy_inline(generic_open)
            + copy_inline(operator_reduction)
            + copy_inline(carrier_stop)
            + copy_inline(surface_role)
            + copy_inline(provenance)
            + copy_inline(merge_role)
    }
}

impl OwnedHeap for FlowInputContext {
    fn owned_heap_bytes(&self, aliases: &mut AliasSet) -> usize {
        let Self {
            contextual_parameters,
        } = self;
        contextual_parameters.owned_heap_bytes(aliases)
    }
}

impl OwnedHeap for RelationPayload {
    /// The binding list with its names and the recursion identity lists.
    fn owned_heap_bytes(&self, aliases: &mut AliasSet) -> usize {
        let Self {
            outcome,
            bindings,
            recursion,
        } = self;
        let outcome = match outcome {
            RelationOutcome::Assignable | RelationOutcome::NotAssignable => 0,
            RelationOutcome::BudgetExceeded(kind) => copy_inline(kind),
        };
        let RelationRecursionFootprint {
            height,
            any,
            unreliable,
            identities,
        } = recursion;
        outcome
            + bindings.owned_heap_bytes(aliases)
            + copy_inline(height)
            + copy_inline(any)
            + copy_inline(unreliable)
            + identities
                .as_ref()
                .map_or(0, |identities| identities.offer(aliases))
    }
}

impl OwnedHeap for RecursionIdentities {
    /// Each side's boxed identity list.
    fn owned_heap_bytes(&self, _aliases: &mut AliasSet) -> usize {
        let Self { sides } = self;
        sides
            .iter()
            .map(|side| {
                verter_session_query::retention::allocation::heap_block_bytes(
                    std::mem::size_of_val::<[(u64, u16)]>(side),
                )
            })
            .sum()
    }
}

impl OwnedHeap for BroadRuntimeClassification {
    fn owned_heap_bytes(&self, aliases: &mut AliasSet) -> usize {
        let Self { kinds, status } = self;
        kinds.owned_heap_bytes(aliases) + copy_inline(status)
    }
}

impl OwnedHeap for MaterializationCycleGateOutcome {
    /// A fallback's reason list.
    fn owned_heap_bytes(&self, aliases: &mut AliasSet) -> usize {
        match self {
            MaterializationCycleGateOutcome::Decided(verdict) => copy_inline(verdict),
            MaterializationCycleGateOutcome::LegacyFallback { verdict, reasons } => {
                let MaterializationCycleGateFallbackReasons(reasons) = reasons;
                copy_inline(verdict) + reasons.offer(aliases)
            }
        }
    }
}

/// A self-root's content hash is inline.
impl OwnedHeap for verter_session_query::analysis::types::Hash16 {
    fn owned_heap_bytes(&self, _aliases: &mut AliasSet) -> usize {
        0
    }
}

#[cfg(test)]
impl FlowReturnResult {
    /// A complete value with `fresh_literal_arms` taken verbatim — no graph
    /// filters them, so a footprint test controls the backing it measures.
    pub(crate) fn for_footprint_tests(
        return_type: SemanticNodeId,
        fresh_literal_arms: verter_session_query::retention::owned::OwnedSlice<SemanticNodeId>,
    ) -> Self {
        Self {
            independent_member: None,
            observed_completeness: crate::semantic_query::ResultCompleteness::Complete,
            exact_return_keys: None,
            return_type,
            can_fall_through:
                verter_session_query::flow::completion::NormalCompletion::minted_for_fixture(false),
            degradation: None,
            fresh_literal_arms,
            facet: FunctionKindFacet::None,
            checker_diagnostics: Vec::new().into(),
            owner: Default::default(),
        }
    }
}
