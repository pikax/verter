//! Retention accounting of candidates that share a build's carrier: the
//! carrier is charged once whichever candidates hold it, and the last
//! surviving holder still pays for it.

use super::*;
use crate::semantic_query::RelationOutcome;
use verter_session_query::facts::fact_cache::{FactVersionRef, ReadSetSignature};
use verter_session_query::retention::allocation::{
    arc_block_bytes, arc_str_list_bytes, fact_slice_bytes, AliasSet,
};
use verter_session_query::retention::shared::registration_bytes;
use verter_session_query::retention::{RetentionLimits, SemanticRetentionAccount, StoreAccount};

const CANONICAL_LEN: usize = 12_000;

fn unbounded_account() -> Arc<SemanticRetentionAccount> {
    SemanticRetentionAccount::new(RetentionLimits {
        aggregate_ceiling_bytes: usize::MAX,
        active_ceiling_bytes: usize::MAX,
        max_entry_bytes: usize::MAX,
        pin_threshold_bytes: usize::MAX,
    })
}

/// A carrier whose one fact names a deliberately oversized canonical: a
/// per-fact proxy would charge it like any short one.
fn oversized_carrier() -> (ReadSetSignature, Arc<[Arc<str>]>) {
    let canonical = format!("/ws/{}.ts", "c".repeat(CANONICAL_LEN));
    let carrier = ReadSetSignature::new(Arc::from(vec![FactVersionRef::FileWholeHash {
        canonical_id: canonical.clone(),
        hash: [7; 16],
    }]));
    let roots: Arc<[Arc<str>]> = Arc::from(vec![Arc::<str>::from(canonical)]);
    (carrier, roots)
}

/// The charges the carrier and self-root list cost, computed from their
/// allocations: each is held once, under one registration.
fn carrier_charge(carrier: &ReadSetSignature, roots: &Arc<[Arc<str>]>) -> usize {
    fact_slice_bytes(&carrier.facts, &mut AliasSet::new())
        + registration_bytes()
        + arc_str_list_bytes(roots, &mut AliasSet::new())
        + registration_bytes()
}

fn stage_relation(
    store: &SemanticGraphStore,
    carrier: &ReadSetSignature,
    roots: &Arc<[Arc<str>]>,
    receipt: &Arc<crate::project_semantic_dispatch::cost_receipt::DemandCostReceipt>,
) -> MemoEntry {
    store.stage_entry(
        SemanticQueryValue::Relation(store.relation_payload_for_tests(RelationOutcome::Assignable)),
        super::relation_memo::relation_satisfied_projection(),
        carrier,
        roots,
        &empty_signature(),
        0,
        receipt,
    )
}

#[test]
fn an_scc_member_shares_the_carrier_charge_and_keeps_it_after_the_root_drains() {
    let (carrier, roots) = oversized_carrier();
    let account = unbounded_account();
    let store = SemanticGraphStore::with_account(StoreAccount::new(Arc::clone(&account)));
    let receipt = crate::project_semantic_dispatch::cost_receipt::DemandCostReceipt::new(
        crate::project_semantic_dispatch::cost_receipt::CostIdentity::new(b"scc-root".to_vec()),
        Default::default(),
        Vec::new(),
    );
    let root = stage_relation(&store, &carrier, &roots, &receipt);
    let member = stage_relation(&store, &carrier, &roots, &receipt);

    let root_retention = store
        .reserve_memo_candidate(&root, 1, 1)
        .expect("the root is admitted");
    let carrier_bytes = carrier_charge(&carrier, &roots);
    assert!(
        carrier_bytes > 2 * CANONICAL_LEN,
        "the oversized canonical is charged in full in the fact rail and the self-root list"
    );
    let after_root = account.snapshot().retained_bytes;
    assert!(after_root >= carrier_bytes + root_retention.own_bytes());

    let members = store
        .reserve_scc_batch(&[&member], 1)
        .expect("the member's incremental footprint is admitted");
    let member_own = super::retention::own_reservation_bytes(&member, 1, 1);
    // What the member newly retains, from its allocations alone: its own
    // bookkeeping and its own (empty) relation-binding value under one
    // registration. The carrier and self-root list are the root's already.
    let member_value = arc_block_bytes(0) + registration_bytes();
    let delta = account.snapshot().retained_bytes - after_root;
    assert_eq!(
        delta,
        member_own + member_value,
        "the member pays its own bookkeeping and its own value — never the shared \
         carrier or self-root list a second time"
    );
    assert!(
        delta < CANONICAL_LEN,
        "a second charge of either shared allocation would cost its oversized canonical"
    );
    assert!(
        members[0]
            .shared_holds()
            .filter(|hold| root_retention
                .shared_holds()
                .any(|root_hold| root_hold.shares_charge_with(hold)))
            .count()
            == 2,
        "the member holds the root's carrier and self-root charges"
    );

    drop(root_retention);
    let survivor = account.snapshot().retained_bytes;
    assert!(
        survivor >= carrier_bytes + member_own,
        "the surviving member still pays for the carrier it keeps reachable: \
         {survivor} < {carrier_bytes} + {member_own}"
    );

    drop(members);
    drop((root, member));
    drop(receipt);
    assert_eq!(account.snapshot().total_bytes(), 0, "every charge released");
    assert_eq!(store.shared_allocation_registrations_for_tests(), 0);
}

#[test]
fn a_refused_candidate_keeps_no_charge_and_no_registration() {
    let (carrier, roots) = oversized_carrier();
    let account = SemanticRetentionAccount::new(RetentionLimits {
        aggregate_ceiling_bytes: CANONICAL_LEN,
        active_ceiling_bytes: CANONICAL_LEN,
        max_entry_bytes: CANONICAL_LEN,
        pin_threshold_bytes: CANONICAL_LEN,
    });
    let store = SemanticGraphStore::with_account(StoreAccount::new(Arc::clone(&account)));
    let receipt = crate::project_semantic_dispatch::cost_receipt::DemandCostReceipt::new(
        crate::project_semantic_dispatch::cost_receipt::CostIdentity::new(b"refused".to_vec()),
        Default::default(),
        Vec::new(),
    );
    let root = stage_relation(&store, &carrier, &roots, &receipt);
    receipt
        .reserve_retention(&account)
        .expect("the receipt alone fits");
    let before = account.snapshot().retained_bytes;
    store
        .reserve_memo_candidate(&root, 1, 1)
        .expect_err("the oversized carrier does not fit, whatever the entry's own size");
    assert_eq!(
        account.snapshot().retained_bytes,
        before,
        "a refusal leaves nothing charged and nothing registered"
    );
    assert_eq!(store.shared_allocation_registrations_for_tests(), 0);
}
