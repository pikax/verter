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
    PrimitiveKind, RelateMemoKey, RelationContext, RelationOutcome, SemanticNodeData,
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
    (published, resident)
}

/// A batch must be ALL-OR-NONE with respect to its own witnessed root
/// and its own members: either every member publishes with the root
/// still resident, or nothing publishes at all. A suffix is forbidden.
///
/// The defect this discriminates: the root witness is checked ONCE, then
/// each member's `record_family_admission_locked` drives the GLOBAL FIFO
/// budget — under pressure that eviction selects the batch's own root and
/// its own earlier members, so the batch returns `true` having published
/// a proper suffix of a component whose root it destroyed. The
/// order-reversal leg pins the second symptom: which keys survive depends
/// on member ORDER, which a component publish must never expose.
///
/// Mutation recipe: removing the batch-scoped eviction exemption (or the
/// oversized-batch refusal) restores `published == true` with a
/// root-less, order-dependent resident set.
#[test]
fn scc_batch_never_evicts_its_own_root_or_members_under_retention_pressure() {
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
/// Mutation recipe: removing the exemption makes the second member's
/// admission pop the root — `root must survive the batch's own
/// admissions` fails while `published` is still `true`.
#[test]
fn scc_batch_evicts_unrelated_families_before_its_own_component() {
    let cap = 4;
    let store = SemanticGraphStore::new_with_memo_budget_for_test(cap);
    let keys = distinct_keys(&store, 6);

    let root_key = keys[0].clone();
    let witness = seed_root(&store, &root_key);
    // Two unrelated families admitted AFTER the root, so the root is the
    // OLDEST ledger record when the drain runs — the FIFO's first victim.
    for filler in &keys[4..6] {
        store.insert_relation_payload_for_tests(
            filler.clone(),
            crate::fact_signature_helpers::ReadSetSignature::empty(),
            Arc::from(Vec::<Arc<str>>::new()),
            store.relation_payload_for_tests(RelationOutcome::NotAssignable),
            0,
        );
    }

    let mut pending = Vec::new();
    for key in &keys[1..4] {
        let flight = store
            .begin_inline_relation_flight(key)
            .expect("each member claims its vacant family flight");
        pending.push(PendingRelationMember {
            key: key.clone(),
            payload: store.relation_payload_for_tests(RelationOutcome::Assignable),
            flight,
        });
    }

    assert!(
        store.publish_scc_members_fenced(
            None,
            &witness,
            &crate::fact_signature_helpers::ReadSetSignature::empty(),
            &Arc::from(Vec::<Arc<str>>::new()),
            0,
            pending,
            Vec::new(),
        ),
        "root + 3 members exactly fills a cap-4 budget and must publish whole"
    );

    for (label, key) in [
        ("root", &keys[0]),
        ("m0", &keys[1]),
        ("m1", &keys[2]),
        ("m2", &keys[3]),
    ] {
        assert_eq!(
            store.slot_candidate_count_for_tests(&key.to_query_key()),
            1,
            "{label} must survive the batch's own admissions"
        );
    }
    let surviving_fillers = [&keys[4], &keys[5]]
        .iter()
        .filter(|key| store.slot_candidate_count_for_tests(&key.to_query_key()) > 0)
        .count();
    assert_eq!(
        surviving_fillers, 0,
        "both unrelated families are the correct FIFO victims — the batch's own \
         component must never be selected ahead of them"
    );
    assert_eq!(
        store.memo_budget_tracked_len_for_test(),
        cap,
        "the ledger must land back exactly at cap, not overshoot"
    );
}
