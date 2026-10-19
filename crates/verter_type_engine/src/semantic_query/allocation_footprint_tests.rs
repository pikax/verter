use std::sync::Arc;

use super::*;
use crate::project_semantic_dispatch::walk::{DiagnosticError, WalkerDiagnostics};
use crate::semantic_query::demand::{Demand, MaterializedPoint};
use verter_session_query::retention::allocation::arc_block_bytes;
use verter_session_query::retention::owned::{OwnedSlice, OwnedStr};

fn arm(name: &str, owner: &str) -> ShallowDiagnostic {
    ShallowDiagnostic::UnresolvedSurfaceArm {
        name: name.into(),
        owner_canonical: owner.into(),
        owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
    }
}

#[test]
fn a_diagnostic_rail_is_charged_for_the_text_its_diagnostics_name() {
    let short = WalkerDiagnostics::new(vec![arm("A", "/a.ts")]);
    let long = WalkerDiagnostics::new(vec![arm(&"n".repeat(9_000), "/a.ts")]);
    let short_bytes = short.owned_heap_bytes(&mut AliasSet::estimating());
    let long_bytes = long.owned_heap_bytes(&mut AliasSet::estimating());
    // The former per-diagnostic proxy charged both 256 bytes.
    assert!(long_bytes >= short_bytes + 8_900);
}

#[test]
fn a_nested_error_and_its_declaration_are_charged() {
    let decl = DeclIdentity {
        canonical_id: OwnedStr::from("/decl.ts"),
        owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
        whole_hash: Default::default(),
        decl_name: OwnedStr::from("Decl"),
    };
    let message: Arc<str> = Arc::from("m".repeat(5_000));
    let diagnostic = ShallowDiagnostic::InstantiationError {
        decl,
        error: DiagnosticError::for_tests(QueryError::Other(verter_type_expr::OwnedStr::from(
            message.clone(),
        ))),
    };
    assert!(diagnostic.owned_heap_bytes(&mut AliasSet::estimating()) >= 5_000);
}

#[test]
fn a_materialised_point_is_charged_for_its_path_text() {
    let path = ProjectionPath::from_segments([PathSegment::Member(PropertyKey::String(
        OwnedStr::from("p".repeat(4_000)),
    ))]);
    let mut demand = Demand::identity();
    demand.projection.path = path;
    let set = MaterializedSet(Arc::from(vec![MaterializedPoint::new(demand)]));
    let empty = MaterializedSet::empty();
    assert!(
        set.owned_heap_bytes(&mut AliasSet::estimating())
            >= empty.owned_heap_bytes(&mut AliasSet::estimating()) + 4_000,
        "the former per-point proxy charged 96 bytes whatever the path"
    );
}

#[test]
fn one_charge_counts_a_shared_allocation_once() {
    let name = OwnedStr::from("s".repeat(3_000));
    let rail = WalkerDiagnostics::new(vec![ShallowDiagnostic::UnresolvedSurfaceArm {
        name: name.clone(),
        owner_canonical: name.clone(),
        owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
    }]);
    let bytes = rail.owned_heap_bytes(&mut AliasSet::estimating());
    assert!(bytes >= 3_000);
    assert!(bytes < 6_000, "one name aliased twice is one allocation");
}

use crate::semantic_query::SemanticNodeId;

fn flow_key(canonical: Arc<str>) -> crate::semantic_query::FlowReturnKey {
    crate::semantic_query::FlowReturnKey {
        function: flow_slot(canonical),
        normalized_type_args: verter_type_expr::OwnedSlice::from(Vec::new().into_boxed_slice()),
        demand: crate::semantic_query::ReturnProjectionDemand::whole_return(),
        input: crate::semantic_query::FlowInputContext::empty(),
        context: crate::semantic_query::FlowReturnContext {
            parse_env_hash: crate::semantic_query::HashValue::default(),
            resolve_env_hash: crate::semantic_query::HashValue::default(),
            type_env_hash: crate::semantic_query::HashValue::default(),
            lib_env_hash: crate::semantic_query::HashValue::default(),
            project_identity: crate::semantic_query::HashValue::default(),
            result_evaluation: crate::semantic_query::CONTEXT_FREE_EVALUATION,
            type_substitution: crate::semantic_query::CanonicalTypeSubstitution::empty(),
            policy: verter_session_query::flow::policy::FlowReturnPolicy {
                nullability: verter_session_query::flow::policy::NullabilityPolicy::Strict,
                no_implicit_any: true,
                use_unknown_in_catch_variables: true,
                no_implicit_this: true,
            },
        },
        result_contract:
            crate::project_semantic_dispatch::flow_solve::flow_return_result_contract_id(),
    }
}

fn flow_slot(canonical: Arc<str>) -> crate::semantic_query::FlowFunctionSlotIdentity {
    crate::semantic_query::FlowFunctionSlotIdentity {
        declaration_slot: crate::semantic_query::ResolvedDeclSlotIdentity::value_slot(
            canonical,
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            Arc::from("f"),
            0,
            crate::semantic_query::HashValue::default(),
            crate::semantic_query::HashValue::default(),
        ),
        function_part: verter_type_expr::facts::FunctionPartIdentity::DeclarationBody,
        overload_ordinal: 0,
    }
}

fn nodes(count: u64) -> OwnedSlice<SemanticNodeId> {
    (0..count).map(SemanticNodeId).collect()
}

fn value_bytes(value: SemanticQueryValue) -> usize {
    value.owned_heap_bytes(&mut AliasSet::estimating())
}

/// The backing an owned slice of `count` elements adds over an empty one.
fn slice_growth<T>(count: usize) -> usize {
    heap_block_bytes(count * std::mem::size_of::<T>())
}

/// A printer for a message that prints no type.
struct NoTypes;

impl crate::semantic_query::CheckerDiagnosticPrinter for NoTypes {
    fn relation_operands(&self, _: SemanticNodeId, _: SemanticNodeId) -> Option<(String, String)> {
        unreachable!("the message has no type argument")
    }
    fn relation_failure(
        &self,
        _: SemanticNodeId,
        _: SemanticNodeId,
        _: crate::semantic_query::CheckerDiagnostic,
        _: &verter_type_expr::IndexedReportSite,
    ) -> Option<Vec<crate::semantic_query::CheckerDiagnosticOccurrence>> {
        unreachable!("the message reports no relation")
    }
}

#[test]
fn a_flow_return_is_charged_for_its_nested_backing() {
    use crate::project_semantic_dispatch::flow_return::{ExactMemberValue, ExactReturnKeyDomain};
    use crate::semantic_query::{
        CheckerDiagnosticArguments, CheckerDiagnosticCode, CheckerDiagnosticOccurrence,
        CheckerDiagnosticRecipe, CheckerDiagnosticSite, CheckerDiagnosticSubject, FlowReturnResult,
    };

    const ARMS: usize = 5_000;
    const TEXT: usize = 9_000;
    let canonical: Arc<str> = Arc::from("/a.ts");
    let small = FlowReturnResult::for_footprint_tests(SemanticNodeId(1), nodes(0));
    let recipe = CheckerDiagnosticRecipe::call(
        CheckerDiagnosticCode::NoMatchingOverload,
        CheckerDiagnosticArguments::None,
        CheckerDiagnosticSubject::Operation,
    );
    let diagnostics: OwnedSlice<CheckerDiagnosticOccurrence> = (0..ARMS)
        .map(|ordinal| {
            let site = CheckerDiagnosticSite {
                operation_start: Some(ordinal as u32),
                operation_end: Some(ordinal as u32),
                ..Default::default()
            };
            recipe
                .bind(&site, &NoTypes)
                .expect("the site names the call")
                .remove(0)
        })
        .collect();
    let long_key: Arc<str> = Arc::from("k".repeat(TEXT));
    let long_root: Arc<str> = Arc::from("r".repeat(TEXT));
    let large = FlowReturnResult::for_footprint_tests(SemanticNodeId(1), nodes(ARMS as u64))
        .with_checker_diagnostics(diagnostics)
        .with_exact_return_keys(ExactReturnKeyDomain::for_footprint_tests(
            flow_key(Arc::clone(&canonical)),
            verter_type_expr::OwnedSlice::from(vec![long_key]),
            Box::new([]),
        ))
        .with_independent_member(ExactMemberValue::for_footprint_tests(
            flow_key(Arc::clone(&canonical)),
            SemanticNodeId(1),
            Box::from(vec![(verter_type_expr::OwnedStr::from(long_root), [0; 16])]),
        ));

    let small_bytes = value_bytes(SemanticQueryValue::FlowReturn(
        verter_session_query::retention::owned::Owned::new(small),
    ));
    let large_bytes = value_bytes(SemanticQueryValue::FlowReturn(
        verter_session_query::retention::owned::Owned::new(large),
    ));
    let nested = slice_growth::<SemanticNodeId>(ARMS)
        + slice_growth::<CheckerDiagnosticOccurrence>(ARMS)
        + 2 * arc_block_bytes(TEXT);
    assert!(
        large_bytes >= small_bytes + nested,
        "large {large_bytes} must exceed small {small_bytes} by its nested backing {nested}"
    );
}

#[test]
fn a_selected_call_is_charged_for_its_substitution_and_fresh_returns() {
    use crate::semantic_query::{
        CanonicalTypeSubstitution, ResolvedCallResult, SignatureCandidateOrigin,
        SignatureOccurrenceIdentity,
    };

    const PAIRS: usize = 5_000;
    const TEXT: usize = 9_000;
    let selected = |origin, substitution, fresh| {
        SemanticQueryValue::ResolveCall(verter_session_query::retention::owned::Owned::new(
            ResolvedCallResult::Selected {
                selected: origin,
                selected_signature: SemanticNodeId(1),
                substitution,
                return_type: SemanticNodeId(2),
                fresh_literal_returns: fresh,
                recovery_diagnostic: None,
            },
        ))
    };
    let small = selected(
        SignatureCandidateOrigin::Rootless,
        CanonicalTypeSubstitution::new_in_arena(None, Vec::new()),
        nodes(0),
    );
    let large = selected(
        SignatureCandidateOrigin::Authored(SignatureOccurrenceIdentity {
            function: flow_slot(Arc::from("c".repeat(TEXT))),
            signature_ordinal: 0,
        }),
        CanonicalTypeSubstitution::new_in_arena(
            None,
            (0..PAIRS as u64)
                .map(|ordinal| (SemanticNodeId(ordinal), SemanticNodeId(ordinal + 1)))
                .collect(),
        ),
        nodes(PAIRS as u64),
    );

    let small_bytes = value_bytes(small);
    let large_bytes = value_bytes(large);
    let nested = slice_growth::<(SemanticNodeId, SemanticNodeId)>(PAIRS)
        + slice_growth::<SemanticNodeId>(PAIRS)
        + arc_block_bytes(TEXT);
    assert!(
        large_bytes >= small_bytes + nested,
        "large {large_bytes} must exceed small {small_bytes} by its nested backing {nested}"
    );
}

#[test]
fn a_flow_return_counts_an_allocation_it_reaches_twice_once() {
    use crate::project_semantic_dispatch::flow_return::{ExactMemberValue, ExactReturnKeyDomain};
    use crate::semantic_query::FlowReturnResult;

    const TEXT: usize = 6_000;
    let canonical: Arc<str> = Arc::from("/a.ts");
    let shared = OwnedStr::from("s".repeat(TEXT));
    let result = FlowReturnResult::for_footprint_tests(SemanticNodeId(1), nodes(0))
        .with_exact_return_keys(ExactReturnKeyDomain::for_footprint_tests(
            flow_key(Arc::clone(&canonical)),
            OwnedSlice::from(vec![shared.clone(), shared.clone()]),
            Box::new([]),
        ))
        .with_independent_member(ExactMemberValue::for_footprint_tests(
            flow_key(Arc::clone(&canonical)),
            SemanticNodeId(1),
            Box::from(vec![(shared.clone(), [0; 16])]),
        ));
    let bytes = value_bytes(SemanticQueryValue::FlowReturn(
        verter_session_query::retention::owned::Owned::new(result),
    ));
    assert!(bytes >= arc_block_bytes(TEXT));
    assert!(
        bytes < 2 * arc_block_bytes(TEXT),
        "one name reached three times is one allocation: {bytes}"
    );
}

#[test]
fn a_fallback_cycle_gate_is_charged_for_its_reason_list() {
    use crate::semantic_query::{
        MaterializationCycleGateFallbackReason, MaterializationCycleGateFallbackReasons,
        MaterializationCycleGateOutcome, MaterializationCycleGateVerdict,
    };
    let reasons = MaterializationCycleGateFallbackReasons::new([
        MaterializationCycleGateFallbackReason::HopLimit,
    ])
    .expect("one reason");
    let fallback = value_bytes(SemanticQueryValue::MaterializationCycleGate(
        MaterializationCycleGateOutcome::LegacyFallback {
            verdict: MaterializationCycleGateVerdict::Stop,
            reasons,
        },
    ));
    let decided = value_bytes(SemanticQueryValue::MaterializationCycleGate(
        MaterializationCycleGateOutcome::Decided(MaterializationCycleGateVerdict::Stop),
    ));
    assert!(
        fallback
            >= decided
                + heap_block_bytes(std::mem::size_of::<MaterializationCycleGateFallbackReason>())
    );
}

#[test]
fn an_object_projection_is_charged_for_its_excess_candidate_text() {
    use crate::semantic_query::object_spread_projection::evaluator_support;
    use crate::semantic_query::{ExcessEligibility, ObjectProjectionSelector};

    const TEXT: usize = 7_000;
    let formula = |excess| {
        SemanticQueryValue::ObjectProjection(evaluator_support::formula([
            evaluator_support::alternative(evaluator_support::AlternativeInput {
                positive: verter_type_expr::OwnedSlice::from([]),
                selector: ObjectProjectionSelector::Surface,
                closed: true,
                residual_operands: verter_type_expr::OwnedSlice::from([]),
                indeterminate_possible_writes: verter_type_expr::OwnedSlice::from([]),
                indices: verter_type_expr::OwnedSlice::from([]),
                signatures: verter_type_expr::OwnedSlice::from([]),
                excess,
            }),
        ]))
    };
    let eligible = formula(ExcessEligibility::Eligible {
        direct_candidates: verter_type_expr::OwnedSlice::from(vec![PropertyKey::String(
            OwnedStr::from("x".repeat(TEXT)),
        )]),
    });
    let indeterminate = formula(ExcessEligibility::Indeterminate);
    assert!(value_bytes(eligible) >= value_bytes(indeterminate) + arc_block_bytes(TEXT));
}

#[test]
fn a_relation_is_charged_for_its_recursion_identities() {
    use crate::semantic_query::{
        RecursionIdentities, RelationOutcome, RelationPayload, RelationRecursionFootprint,
    };

    const IDENTITIES: usize = 4_000;
    let relation = |identities| {
        SemanticQueryValue::Relation(RelationPayload {
            outcome: RelationOutcome::Assignable,
            bindings: Vec::new().into(),
            recursion: RelationRecursionFootprint {
                identities,
                ..Default::default()
            },
        })
    };
    let side: Box<[(u64, u16)]> = (0..IDENTITIES as u64).map(|id| (id, 1)).collect();
    let with = relation(Some(verter_session_query::retention::owned::Owned::new(
        RecursionIdentities {
            sides: [side.clone(), side],
        },
    )));
    let without = relation(None);
    let lists = 2 * heap_block_bytes(IDENTITIES * std::mem::size_of::<(u64, u16)>());
    assert!(value_bytes(with) >= value_bytes(without) + lists);
}

#[test]
fn a_diagnostic_chain_is_charged_one_block_per_allocation() {
    use crate::semantic_query::{
        CheckerDiagnostic, CheckerDiagnosticChainLine, CheckerDiagnosticCode,
        CheckerDiagnosticOccurrence, CheckerDiagnosticOperation,
    };
    use verter_session_query::retention::allocation::heap_block_bytes;

    const TEXT: usize = 37;
    let occurrence = |chain: Vec<CheckerDiagnosticChainLine>| {
        CheckerDiagnosticOccurrence::new(
            0,
            0,
            CheckerDiagnostic {
                code: CheckerDiagnosticCode::TypeNotAssignable,
                operation: CheckerDiagnosticOperation::CallResolution,
            },
            Box::new([]),
            chain.into_boxed_slice(),
        )
    };
    let bare = occurrence(Vec::new()).heap_bytes(&mut AliasSet::estimating());
    let chained = occurrence(vec![
        CheckerDiagnosticChainLine::new(1, "a".repeat(TEXT).into_boxed_str()),
        CheckerDiagnosticChainLine::new(2, "b".repeat(TEXT).into_boxed_str()),
    ])
    .heap_bytes(&mut AliasSet::estimating());
    let line = std::mem::size_of::<CheckerDiagnosticChainLine>();
    assert_eq!(
        chained - bare,
        heap_block_bytes(2 * line) + 2 * heap_block_bytes(TEXT),
        "the chain backing and each message are separate allocations"
    );
}
