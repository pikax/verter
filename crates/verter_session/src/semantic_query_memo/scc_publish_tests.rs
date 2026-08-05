//! Batched SCC member publication under GLOBAL retention pressure.
//!
//! The batch's own admissions drive the shared `memo_budget` FIFO. Under
//! pressure those admissions can select a victim, and the two victims
//! that must never be selectable are the batch's OWN witnessed root and
//! the batch's OWN already-written members: publishing a suffix of a
//! component onto a root the same publish just evicted is the
//! partially-published component the root-witness fence exists to
//! forbid, arriving through a different door.

use super::*;
use crate::semantic_query::{
    CanonicalTypeSubstitution, FlowFunctionSlotIdentity, FlowInputContext, FlowReturnContext,
    FlowReturnKey, FlowReturnPolicy, FlowReturnResult, PrimitiveKind, RelateMemoKey,
    RelationContext, RelationOutcome, ResolvedDeclSlotIdentity, ReturnProjectionDemand,
    SemanticNodeData,
};

/// Seed a decided root candidate and return the witness a batch fences on.
fn seed_root(store: &SemanticGraphStore, key: &RelateMemoKey) -> SccRootWitness {
    store.insert_relation_payload_for_tests(
        key.clone(),
        crate::fact_signature_helpers::ReadSetSignature::empty(),
        Arc::from(Vec::<Arc<str>>::new()),
        store.relation_payload_for_tests(RelationOutcome::Assignable),
        0,
    );
    let admission_seq = store
        .relation_published_carrier(key)
        .expect("the seeded root publishes a carrier")
        .admission_seq;
    SccRootWitness::relate(key.clone(), admission_seq)
}

/// Distinct relation identities over one shared target, so every member
/// keys a distinct `FamilyKey::Relate` family.
fn distinct_keys(store: &SemanticGraphStore, count: usize) -> Vec<RelateMemoKey> {
    let target = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Unknown));
    (0..count)
        .map(|index| {
            let source = store.intern_node(SemanticNodeData::Literal(
                crate::semantic_query::LiteralValue::String(format!("scc-member-{index}")),
            ));
            RelateMemoKey::assignable(source, target, RelationContext::default())
        })
        .collect()
}

/// Distinct flow-return identities over one shared canonical, so every
/// member keys a distinct `FamilyKey::FlowReturn` family — the OTHER
/// domain the batched publish serves.
fn distinct_flow_keys(count: usize) -> Vec<FlowReturnKey> {
    (0..count)
        .map(|index| FlowReturnKey {
            function: FlowFunctionSlotIdentity {
                declaration_slot: ResolvedDeclSlotIdentity::value_slot(
                    Arc::from("/ws/scc-flow.ts"),
                    verter_type_expr::TopLevelOwnerId::ordinary_file(),
                    Arc::from(format!("sccFlowMember{index}")),
                    0,
                    [0u8; 16],
                    [0u8; 16],
                ),
                function_part: verter_type_expr::facts::FunctionPartIdentity::DeclarationBody,
                overload_ordinal: 0,
            },
            normalized_type_args: Arc::from(Vec::new().into_boxed_slice()),
            context: FlowReturnContext {
                parse_env_hash: [0u8; 16],
                resolve_env_hash: [0u8; 16],
                type_env_hash: [0u8; 16],
                lib_env_hash: [0u8; 16],
                project_identity: [0u8; 16],
                type_substitution: CanonicalTypeSubstitution::empty(),
                policy: FlowReturnPolicy {},
            },
            demand: ReturnProjectionDemand::whole_return(),
            input: FlowInputContext::empty(),
        })
        .collect()
}

/// The whole-return point a flow member's compute materialises.
fn flow_whole_return_projection() -> MaterializedSet {
    MaterializedSet::single(MaterializedPoint::new(family::point_for_slot(
        ModeSlot::Single,
        &ProjectionPath::empty(),
    )))
}

/// Stage `count` CLEAN flow-return members, each claiming its own vacant
/// family flight.
fn pending_flow_members(
    store: &SemanticGraphStore,
    keys: &[FlowReturnKey],
) -> Vec<PendingFlowReturnMember> {
    let return_type = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    keys.iter()
        .map(|key| {
            let flight = store
                .begin_inline_flow_return_flight(key)
                .expect("each flow member claims its vacant family flight");
            PendingFlowReturnMember {
                key: key.clone(),
                result: FlowReturnResult {
                    return_type,
                    can_fall_through: false,
                    degradation: None,
                },
                materialized: flow_whole_return_projection(),
                flight,
            }
        })
        .collect()
}

/// Run one batch of `members` (in the given order) against a store whose
/// global family budget is `cap`, and report `(published, resident)`.
fn run_batch(cap: usize, member_order: &[usize]) -> (bool, Vec<String>) {
    let store = SemanticGraphStore::new_with_memo_budget_for_test(cap);
    let keys = distinct_keys(&store, member_order.len() + 1);
    let root_key = keys[0].clone();
    let witness = seed_root(&store, &root_key);

    let mut pending = Vec::new();
    for index in member_order {
        let key = keys[*index + 1].clone();
        let flight = store
            .begin_inline_relation_flight(&key)
            .expect("each member claims its vacant family flight");
        pending.push(PendingRelationMember {
            key,
            payload: store.relation_payload_for_tests(RelationOutcome::Assignable),
            flight,
        });
    }

    let published = store.publish_scc_members_fenced(
        None,
        &witness,
        &crate::fact_signature_helpers::ReadSetSignature::empty(),
        &Arc::from(Vec::<Arc<str>>::new()),
        0,
        pending,
        Vec::new(),
    );

    // Residency is reported by STABLE NAME (`root`, `m0`, `m1`, ...), not
    // by publication order, so the two orderings' reports are directly
    // comparable.
    let mut resident = Vec::new();
    if store.slot_candidate_count_for_tests(&root_key.to_query_key()) > 0 {
        resident.push("root".to_string());
    }
    for index in 0..member_order.len() {
        if store.slot_candidate_count_for_tests(&keys[index + 1].to_query_key()) > 0 {
            resident.push(format!("m{index}"));
        }
    }

    assert!(
        store.retained_claimed_flight_keys_for_tests().is_empty(),
        "every member flight must be released either way: {:?}",
        store.retained_claimed_flight_keys_for_tests()
    );
    assert!(
        store.resident_flight_keys_for_tests().is_empty(),
        "every member flight must be retired either way: {:?}",
        store.resident_flight_keys_for_tests()
    );
    // The batch's exempting budget step must never leave the ledger above
    // its bound. This is the DIRECT reading of the pinning failure the
    // whole-component refusal exists to prevent: an exempt set wider than
    // the cap makes every record unselectable, so the trim settles ABOVE
    // `cap` and stays there. Residency alone reads it only by proxy.
    assert!(
        store.memo_budget_tracked_len_for_test() <= cap,
        "the retention ledger must stay within its cap (tracked={}, cap={cap})",
        store.memo_budget_tracked_len_for_test()
    );
    (published, resident)
}

/// A batch must be ALL-OR-NONE with respect to its own witnessed root
/// and its own members: either every member publishes with the root
/// still resident, or nothing publishes at all. A suffix is forbidden.
///
/// The defect this discriminates: a component whose resident footprint —
/// its root PLUS every member — cannot fit the global family budget is
/// published anyway. It cannot be retained coherently at any cap, and the
/// exemption that would keep it whole is wider than the ledger's bound,
/// so the trim settles permanently ABOVE the cap. The order-reversal legs
/// pin the second symptom: which keys survive a refused batch must never
/// depend on member ORDER.
///
/// The sibling test owns the other half — the eviction exemption that
/// keeps a FITTING component whole under real pressure.
///
/// Mutation recipe. This test discriminates the WHOLE-COMPONENT REFUSAL,
/// and only that:
///
/// - Removing the footprint refusal (`footprint > cap`) restores
///   `published == true` on the OVERSIZED and BOUNDARY legs, with a
///   root-less, order-dependent resident set and a ledger pinned above
///   its cap by an exempt set wider than the budget.
/// - Dropping the ROOT from the footprint count (`1 + members` →
///   `members`) is caught ONLY by the BOUNDARY leg — root + 3 members
///   against a cap of exactly 3 — where the undercount reads
///   `3 > 3 == false` and admits a component the ledger cannot hold. The
///   OVERSIZED leg (cap 2) still refuses under the undercount, so it
///   cannot see this.
///
/// The batch-scoped eviction EXEMPTION is deliberately NOT discriminated
/// here, and a recipe claiming otherwise is false: the OVERSIZED and
/// BOUNDARY legs never reach the budget step at all (the footprint gate
/// refuses first), and the FITTING leg puts 4 families against a cap of 8
/// — zero pressure, so no victim is ever selected. Removing the exemption
/// outright leaves this test GREEN. Its owner is the sibling
/// [`scc_batch_evicts_unrelated_families_before_its_own_component`],
/// whose exact-fit batch runs under real pressure.
#[test]
fn scc_batch_never_evicts_its_own_root_or_members_under_retention_pressure() {
    // BOUNDARY — root + 3 members needs 4 resident families against a cap
    // of exactly 3. This is the only leg that sits ON the footprint
    // predicate: the members alone fit, the component does not. Both
    // orders must refuse WHOLE, and the ledger (asserted inside
    // `run_batch`) must not end up pinned above the cap by an
    // over-wide exemption.
    let (published_boundary, resident_boundary) = run_batch(3, &[0, 1, 2]);
    let (published_boundary_rev, resident_boundary_rev) = run_batch(3, &[2, 1, 0]);
    assert!(
        !published_boundary && !published_boundary_rev,
        "a component whose ROOT is what pushes it past the cap must refuse WHOLE \
         (forward={published_boundary}, reverse={published_boundary_rev})"
    );
    assert_eq!(
        resident_boundary,
        vec!["root".to_string()],
        "the boundary batch publishes no member and leaves its witnessed root resident"
    );
    assert_eq!(
        resident_boundary, resident_boundary_rev,
        "member ORDER must never change which keys stay warm"
    );

    // OVERSIZED — root + 3 members needs 4 resident families against a
    // cap of 2. The component cannot be retained coherently, so NOTHING
    // publishes and the pre-existing root survives untouched.
    let (published_forward, resident_forward) = run_batch(2, &[0, 1, 2]);
    let (published_reverse, resident_reverse) = run_batch(2, &[2, 1, 0]);
    assert!(
        !published_forward && !published_reverse,
        "a component that cannot fit the retention budget must refuse WHOLE \
         (forward={published_forward}, reverse={published_reverse})"
    );
    assert_eq!(
        resident_forward,
        vec!["root".to_string()],
        "a refused batch publishes no member and leaves its witnessed root resident"
    );
    assert_eq!(
        resident_forward, resident_reverse,
        "member ORDER must never change which keys stay warm"
    );

    // FITTING — root + 3 members against a cap of 8. The whole component
    // publishes, root included, identically in both orders.
    let (published_fit, resident_fit) = run_batch(8, &[0, 1, 2]);
    let (published_fit_rev, resident_fit_rev) = run_batch(8, &[2, 1, 0]);
    assert!(
        published_fit && published_fit_rev,
        "a component that fits must publish WHOLE \
         (forward={published_fit}, reverse={published_fit_rev})"
    );
    assert_eq!(
        resident_fit,
        vec![
            "root".to_string(),
            "m0".to_string(),
            "m1".to_string(),
            "m2".to_string()
        ],
        "every member publishes onto a root that is still resident"
    );
    assert_eq!(
        resident_fit, resident_fit_rev,
        "member ORDER must never change which keys stay warm"
    );
}

/// Which component family the pressure shape makes ALREADY RESIDENT
/// before the drain, ahead of the unrelated fillers.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PreResident {
    /// Only the root is resident — every member family is newly keyed by
    /// the batch, so every member's ledger record is NEWER than the
    /// fillers'. This shape can only observe the ROOT's exemption.
    RootOnly,
    /// The root PLUS the component's first member. An already-resident
    /// member family carries an OLD ledger record — older than the
    /// fillers — which the batch's write does NOT refresh (the write is
    /// not newly-keying, so it records no new admission). It is therefore
    /// the FIRST thing the batch's own budget step would select, and it is
    /// the only shape in which a MEMBER's exemption is observable at all.
    RootAndFirstMember,
}

/// Run the exact-fit pressure shape for one domain mix: a relation root
/// plus `relation_members` relation members and `flow_members`
/// flow-return members, against a budget that the component fills
/// EXACTLY, with two unrelated families admitted after the root (and
/// after the pre-resident member, when there is one).
fn run_exact_fit_pressure_batch(
    relation_members: usize,
    flow_members: usize,
    pre_resident: PreResident,
) {
    let cap = 1 + relation_members + flow_members;
    let store = SemanticGraphStore::new_with_memo_budget_for_test(cap);
    // keys[0] = root, keys[1..=relation_members] = relation members,
    // the trailing two = the unrelated fillers.
    let keys = distinct_keys(&store, relation_members + 3);
    let filler_start = relation_members + 1;
    let flow_keys = distinct_flow_keys(flow_members);

    let root_key = keys[0].clone();
    let witness = seed_root(&store, &root_key);

    // The already-resident member lands BEFORE the fillers, so its ledger
    // record is older than theirs.
    // A FLOW member is pre-resident whenever the mix has one: the flow
    // domain is the half no relation-only leg can observe.
    if pre_resident == PreResident::RootAndFirstMember {
        if flow_members > 0 {
            let return_type = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
            store.publish_unfenced_candidate_for_tests(
                None,
                FamilyKey::FlowReturn {
                    key: Box::new(flow_keys[0].clone()),
                },
                SemanticQueryValue::FlowReturn(Arc::new(FlowReturnResult {
                    return_type,
                    can_fall_through: false,
                    degradation: None,
                })),
                flow_whole_return_projection(),
                crate::fact_signature_helpers::ReadSetSignature::empty(),
                Arc::from(Vec::<Arc<str>>::new()),
                0,
            );
        } else {
            store.insert_relation_payload_for_tests(
                keys[1].clone(),
                crate::fact_signature_helpers::ReadSetSignature::empty(),
                Arc::from(Vec::<Arc<str>>::new()),
                store.relation_payload_for_tests(RelationOutcome::Assignable),
                0,
            );
        }
    }

    // Two unrelated families admitted AFTER the root, so the root is the
    // OLDEST ledger record when the drain runs — the FIFO's first victim.
    for filler in &keys[filler_start..] {
        store.insert_relation_payload_for_tests(
            filler.clone(),
            crate::fact_signature_helpers::ReadSetSignature::empty(),
            Arc::from(Vec::<Arc<str>>::new()),
            store.relation_payload_for_tests(RelationOutcome::NotAssignable),
            0,
        );
    }

    let mut pending = Vec::new();
    for key in &keys[1..filler_start] {
        let flight = store
            .begin_inline_relation_flight(key)
            .expect("each member claims its vacant family flight");
        pending.push(PendingRelationMember {
            key: key.clone(),
            payload: store.relation_payload_for_tests(RelationOutcome::Assignable),
            flight,
        });
    }
    let pending_flow = pending_flow_members(&store, &flow_keys);

    let label = format!(
        "{relation_members} relation + {flow_members} flow members, {}",
        match pre_resident {
            PreResident::RootOnly => "root-only pre-resident",
            PreResident::RootAndFirstMember => "first member pre-resident",
        }
    );
    assert!(
        store.publish_scc_members_fenced(
            None,
            &witness,
            &crate::fact_signature_helpers::ReadSetSignature::empty(),
            &Arc::from(Vec::<Arc<str>>::new()),
            0,
            pending,
            pending_flow,
        ),
        "root + {label} exactly fills a cap-{cap} budget and must publish whole"
    );

    assert_eq!(
        store.slot_candidate_count_for_tests(&root_key.to_query_key()),
        1,
        "the root must survive the batch's own admissions ({label})"
    );
    for (index, key) in keys[1..filler_start].iter().enumerate() {
        assert_eq!(
            store.slot_candidate_count_for_tests(&key.to_query_key()),
            1,
            "relation member m{index} must survive the batch's own admissions ({label})"
        );
    }
    for (index, key) in flow_keys.iter().enumerate() {
        assert_eq!(
            store.slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(
                key.clone()
            ))),
            1,
            "flow member f{index} must survive the batch's own admissions ({label})"
        );
    }
    let surviving_fillers = keys[filler_start..]
        .iter()
        .filter(|key| store.slot_candidate_count_for_tests(&key.to_query_key()) > 0)
        .count();
    assert_eq!(
        surviving_fillers, 0,
        "both unrelated families are the correct FIFO victims — the batch's own \
         component must never be selected ahead of them ({label})"
    );
    assert_eq!(
        store.memo_budget_tracked_len_for_test(),
        cap,
        "the ledger must land back exactly at cap, not overshoot ({label})"
    );
    assert!(
        store.retained_claimed_flight_keys_for_tests().is_empty(),
        "every member flight must be released ({label}): {:?}",
        store.retained_claimed_flight_keys_for_tests()
    );
}

/// The production shape: a FITTING batch whose root is no longer the
/// newest ledger record. The root's own transaction publishes further
/// sub-queries between the root's publish and the member drain, so by
/// drain time the root sits BEHIND them in the global FIFO — and the
/// batch's own admissions reach it before they reach the newer,
/// unrelated families.
///
/// The batch fits the budget exactly (root + 3 members == cap 4), so the
/// oversized-batch refusal cannot be what saves it; only the
/// batch-scoped eviction exemption can. Every victim must be an
/// unrelated family.
///
/// The exemption is FAMILY-AGNOSTIC and MANDATORY, so it is exercised in
/// BOTH domains: an all-relation component, an all-FLOW component, and a
/// MIXED one.
///
/// The two pre-residency shapes observe DIFFERENT halves of the exempt
/// set, and only the second can see a member at all. With the root as the
/// only pre-existing component family, every member's ledger record is
/// newer than both fillers', the overflow is exactly the filler count, and
/// no member is ever reachable by victim selection — that shape pins the
/// ROOT's exemption and nothing else. An ALREADY-RESIDENT member is the
/// discriminating shape: its record predates the fillers and the batch's
/// non-newly-keying write does not refresh it, so it is the first
/// selectable record in the ledger. That is exactly why the batch exempts
/// the whole `component` set and not merely the `newly_keyed` subset, and
/// it is the only way a `FamilyKey::FlowReturn` omitted from the exempt
/// set becomes observable.
///
/// Mutation recipe: removing the exemption entirely makes the batch's own
/// admissions pop the root — `the root must survive the batch's own
/// admissions` fails while `published` is still `true`, in every leg.
/// Narrowing the exempt set to `newly_keyed` (dropping the already-
/// resident member) or to the relation domain alone leaves every
/// `RootOnly` leg green and fails the corresponding
/// `RootAndFirstMember` leg.
#[test]
fn scc_batch_evicts_unrelated_families_before_its_own_component() {
    // All-relation, all-flow, and mixed — each filling its cap exactly.
    for pre_resident in [PreResident::RootOnly, PreResident::RootAndFirstMember] {
        run_exact_fit_pressure_batch(3, 0, pre_resident);
        run_exact_fit_pressure_batch(0, 3, pre_resident);
        run_exact_fit_pressure_batch(2, 2, pre_resident);
    }
}
