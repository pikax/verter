//! SCC batch retention accounting: a staged member reserves only its own
//! allocations, never the carrier its witnessed root already owns.

use super::*;
use crate::semantic_query::RelationOutcome;
use verter_session_query::retention::{
    ChargeClass, RetainedFootprint, RetentionLimits, SemanticRetentionAccount,
};

#[test]
fn scc_member_reservation_excludes_the_roots_shared_carrier() {
    use verter_session_query::facts::fact_cache::FactVersionRef;

    let carrier = verter_session_query::facts::fact_cache::ReadSetSignature::new(Arc::from(vec![
        FactVersionRef::FileWholeHash {
            canonical_id: "/ws/scc-accounting.ts".to_string(),
            hash: [7; 16],
        },
    ]));
    let roots: Arc<[Arc<str>]> = Arc::from(vec![Arc::<str>::from("/ws/scc-accounting.ts")]);
    let limits = RetentionLimits {
        aggregate_ceiling_bytes: usize::MAX,
        active_ceiling_bytes: usize::MAX,
        max_entry_bytes: usize::MAX,
        pin_threshold_bytes: usize::MAX,
    };
    let account = SemanticRetentionAccount::new(limits);
    let store = SemanticGraphStore::with_account(
        verter_session_query::retention::StoreAccount::new(Arc::clone(&account)),
    );
    let receipt = crate::project_semantic_dispatch::cost_receipt::DemandCostReceipt::new(
        crate::project_semantic_dispatch::cost_receipt::CostIdentity::new(b"scc-root".to_vec()),
        Default::default(),
        Vec::new(),
    );
    let root = store.stage_entry(
        None,
        SemanticQueryValue::Relation(store.relation_payload_for_tests(RelationOutcome::Assignable)),
        super::relation_memo::relation_satisfied_projection(),
        &carrier,
        &roots,
        &empty_signature(),
        0,
        &receipt,
    );
    let member = store.stage_entry(
        None,
        SemanticQueryValue::Relation(store.relation_payload_for_tests(RelationOutcome::Assignable)),
        super::relation_memo::relation_satisfied_projection(),
        &carrier,
        &roots,
        &empty_signature(),
        0,
        &receipt,
    );
    let root_charge = account
        .reserve(ChargeClass::Retained, root.retained_footprint_bytes())
        .admitted()
        .expect("the root is admitted");
    let member_unique = member.unique_retained_footprint_bytes();
    let batch = store
        .reserve_scc_batch(&[&member])
        .expect("the member's incremental footprint is admitted");

    assert_eq!(
        account.snapshot().retained_bytes,
        root_charge.bytes() + member_unique,
        "the root already owns the shared carrier and self-root allocations"
    );
    drop(batch);
    drop(root_charge);
}
