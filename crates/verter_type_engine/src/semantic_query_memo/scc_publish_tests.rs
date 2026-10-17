//! Retention accounting of candidates that share a build's carrier: the
//! carrier is charged once whichever candidates hold it, and the last
//! surviving holder still pays for it.

use super::*;
use crate::semantic_query::RelationOutcome;
use verter_session_query::facts::fact_cache::{FactVersionRef, ReadSetSignature};
use verter_session_query::retention::allocation::{arc_str_list_bytes, fact_slice_bytes, AliasSet};
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

/// A relation family keyed by `source`.
fn relation_family(source: u64) -> FamilyKey {
    FamilyKey::Relate {
        key: super::family_intern::InternedRelateKey::intern(
            None,
            crate::semantic_query::RelateMemoKey::assignable(
                crate::semantic_query::SemanticNodeId(source),
                crate::semantic_query::SemanticNodeId(source + 1),
                crate::semantic_query::RelationContext::default(),
            ),
        ),
    }
}

fn stage_relation(
    store: &SemanticGraphStore,
    carrier: &ReadSetSignature,
    roots: &Arc<[Arc<str>]>,
    receipt: &Arc<crate::project_semantic_dispatch::cost_receipt::DemandCostReceipt>,
) -> MemoEntry {
    store
        .stage_entry(
            SemanticQueryValue::Relation(
                store.relation_payload_for_tests(RelationOutcome::Assignable),
            ),
            super::relation_memo::relation_satisfied_projection(),
            carrier,
            roots,
            &empty_signature(),
            0,
            receipt,
        )
        .expect("a relation payload names no node")
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

    let (root_family, member_family) = (relation_family(0x5C_0001), relation_family(0x5C_0011));
    let root_retention = store
        .reserve_memo_candidate(&root, &root_family, 1, 1)
        .expect("the root is admitted");
    let carrier_bytes = carrier_charge(&carrier, &roots);
    assert!(
        carrier_bytes > 2 * CANONICAL_LEN,
        "the oversized canonical is charged in full in the fact rail and the self-root list"
    );
    let after_root = account.snapshot().retained_bytes;
    assert!(after_root >= carrier_bytes + root_retention.own_bytes());

    let members = store
        .reserve_scc_batch(&[(&member_family, &member)], 1)
        .expect("the member's incremental footprint is admitted");
    let verter_session_query::retention::Footprint {
        bytes: member_own,
        handles,
    } = super::retention::own_reservation(&member, &member_family, 1, 1);
    // The owned allocations its bookkeeping reaches (its satisfied point's
    // path) are charged by their own cells, retained with it.
    let member_own_handles = handles.charged_bytes();
    drop(handles);
    // What the member newly retains, from its allocations alone: its own
    // bookkeeping and its own (empty) relation-binding value, an owned cell
    // charged by itself. The carrier and self-root list are the root's
    // already.
    let member_value = verter_session_query::retention::owned::Owned::<
        Box<[crate::semantic_query::InferBinding]>,
    >::cell_block_bytes();
    // Its delivery's metadata, pinned to the delivery until now, is adopted
    // into retention with it.
    let member_metadata = member.lease.own_charge_bytes_for_tests();
    let delta = account.snapshot().retained_bytes - after_root;
    assert_eq!(
        delta,
        member_own + member_own_handles + member_value + member_metadata,
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
    let before = account.snapshot();
    // The staged entry's own delivery already holds its value as the
    // caller's: that pin and registration predate the admission.
    let registered = store.shared_allocation_registrations_for_tests();
    store
        .reserve_memo_candidate(&root, &relation_family(0x5C_0021), 1, 1)
        .expect_err("the oversized carrier does not fit, whatever the entry's own size");
    let after = account.snapshot();
    assert_eq!(
        (after.retained_bytes, after.pinned_bytes),
        (before.retained_bytes, before.pinned_bytes),
        "a refusal leaves nothing charged and nothing registered"
    );
    assert_eq!(
        store.shared_allocation_registrations_for_tests(),
        registered
    );
}

/// One oversized name shared by a diagnostic and a dependency rail — two
/// different carriers — is charged once, by its own cell, and stays
/// charged, once, while either carrier survives the other.
#[test]
fn a_name_a_diagnostic_and_a_dependency_rail_share_is_charged_once() {
    use crate::project_semantic_dispatch::walk::{ShallowDiagnostic, WalkerDiagnostics};
    use crate::semantic_query::DepVersion;
    use verter_session_query::retention::owned::OwnedStr;
    const NAME: usize = 30_000;
    for diagnostics_first in [true, false] {
        let account = unbounded_account();
        let store = SemanticGraphStore::with_account(StoreAccount::new(Arc::clone(&account)));
        let name = OwnedStr::from("n".repeat(NAME));
        let diagnostics = WalkerDiagnostics::new(vec![ShallowDiagnostic::UnresolvedSurfaceArm {
            name: name.clone(),
            owner_canonical: "/o.ts".into(),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
        }]);
        let dep_signature =
            crate::semantic_query::DepSignature::new(vec![crate::semantic_query::DepEntry(
                name.clone(),
                DepVersion::ProjectGeneration(1),
            )]);
        drop(name);
        let facts = crate::fact_signature_helpers::empty_fact_signature();
        let roots: Arc<[Arc<str>]> = Arc::from(Vec::new());
        let result = QueryResult::Value(SemanticQueryValue::TypeNode(
            crate::semantic_query::SemanticNodeId(0),
        ));
        let holds = store
            .hold_shared_allocations(super::retention::SharedParts {
                lease: &Default::default(),
                facts: &facts,
                self_roots: &roots,
                diagnostics: &diagnostics,
                dep_signature: &dep_signature,
                result: &result,
            })
            .expect("admitted");
        let charged = account.snapshot().retained_bytes;
        let name_charge = OwnedStr::charge_for_len(NAME);
        let without_name = charged - name_charge;
        assert!(
            without_name < NAME,
            "the shared name is charged once, not once per rail: {charged}"
        );
        // Drop one rail's charge, keeping the other through a second holder.
        let (no_diagnostics, no_dependencies) = (
            crate::semantic_query_memo::empty_diagnostics(),
            crate::semantic_query_memo::empty_signature(),
        );
        let survivor = store
            .hold_shared_allocations(super::retention::SharedParts {
                lease: &Default::default(),
                facts: &facts,
                self_roots: &roots,
                diagnostics: if diagnostics_first {
                    &no_diagnostics
                } else {
                    &diagnostics
                },
                dep_signature: if diagnostics_first {
                    &dep_signature
                } else {
                    &no_dependencies
                },
                result: &result,
            })
            .expect("shared");
        drop(holds);
        assert!(
            account.snapshot().retained_bytes >= name_charge,
            "the surviving rail keeps the shared name charged"
        );
        drop(survivor);
        assert_eq!(account.snapshot().retained_bytes, 0);
        assert_eq!(store.shared_allocation_registrations_for_tests(), 0);
    }
}
