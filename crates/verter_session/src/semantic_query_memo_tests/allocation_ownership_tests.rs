//! Retained memo allocations are charged by what they actually allocate,
//! once, and a delivery that outlives the cache keeps the allocations it
//! aliases charged — pinned to its caller — until its final holder drops.
//! The cache's own bookkeeping never outlives the cache.

use std::sync::Arc;

use super::*;
use crate::{HostConfig, UpsertRequest, VerterHost};
use verter_session_query::retention::allocation::heap_block_bytes;
use verter_session_query::retention::owned::{Owned, OwnedStr};
use verter_session_query::retention::{
    ChargeClass, RetentionAccountSnapshot, RetentionLimits, SemanticRetentionAccount, StoreAccount,
};
use verter_type_engine::project_semantic_dispatch::walk::{QueryBuildOutput, ShallowDiagnostic};
use verter_type_engine::semantic_query::{
    CacheRead, LiteralValue, PrimitiveKind, ResolveDeclKey, ScopeId,
};
use verter_type_engine::semantic_query_memo::graph_fixture::GraphFixture;
use verter_type_expr::TopLevelOwnerId;

const FILE: &str = "/allocation/owned.ts";
const OWNER: &str = "/allocation/owner.ts";
const NAME_LEN: usize = 20_000;

fn host() -> VerterHost {
    let host = VerterHost::new_standalone(HostConfig::default());
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(FILE.to_string()),
            input_id: FILE.to_string(),
            source: Arc::from("export interface Target { base: number; }\n"),
            file_language: crate::LanguageRegistry::global()
                .classify_static(FILE)
                .static_resolution(),
            aliases: Vec::new(),
        })
        .expect("upsert of the fixture file succeeds");
    host
}

fn key() -> SemanticQueryKey {
    SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: ScopeId {
            canonical_id: Arc::from(FILE),
            owner: TopLevelOwnerId::ordinary_file(),
            local_scope: None,
            binder_scope_id: verter_type_engine::semantic_query::BinderScopeId::file_scope(
                TopLevelOwnerId::ordinary_file(),
            ),
        },
        name: Arc::from("Target"),
    })
}

fn unbounded() -> Arc<SemanticRetentionAccount> {
    SemanticRetentionAccount::new(RetentionLimits {
        aggregate_ceiling_bytes: usize::MAX / 2,
        active_ceiling_bytes: usize::MAX / 2,
        max_entry_bytes: usize::MAX / 2,
        pin_threshold_bytes: usize::MAX / 2,
    })
}

/// A complete build whose one walker diagnostic names a deliberately
/// oversized arm: the rail a warm read hands its caller.
fn build(store: &SemanticGraphStore) -> QueryBuildOutput<SemanticNodeId> {
    let mut output: QueryBuildOutput<SemanticNodeId> = (
        QueryResult::Value(
            store.intern_node(SemanticNodeData::Literal(LiteralValue::String(
                "owned".into(),
            ))),
        ),
        empty_signature(),
    )
        .into();
    output.walker_diagnostics = vec![ShallowDiagnostic::UnresolvedSurfaceArm {
        name: verter_session_query::retention::owned::OwnedStr::from("n".repeat(NAME_LEN)),
        owner_canonical: verter_session_query::retention::owned::OwnedStr::from(OWNER),
        owner: TopLevelOwnerId::ordinary_file(),
    }];
    output
}

/// What the delivered rail costs, from its allocations alone: the list's
/// cell and the backing holding the one diagnostic, and the two texts it
/// names, each its own self-charged allocation. A charge missing the name,
/// or a second charge of any of them, misses this figure.
fn rail_charge() -> usize {
    Owned::<Box<[ShallowDiagnostic]>>::cell_block_bytes()
        + heap_block_bytes(std::mem::size_of::<ShallowDiagnostic>())
        + OwnedStr::charge_for_len(NAME_LEN)
        + OwnedStr::charge_for_len(OWNER.len())
}

/// The oversized name the delivered diagnostic carries: an owning clone of
/// its allocation.
fn delivered_name<T>(read: &CacheRead<T>) -> OwnedStr {
    match read.walker_diagnostics() {
        [ShallowDiagnostic::UnresolvedSurfaceArm { name, .. }] => name.clone(),
        other => panic!("one delivered arm diagnostic, got {other:?}"),
    }
}

fn read(harness: &GraphFixture, host: &VerterHost) -> CacheRead<QueryResult<SemanticNodeId>> {
    harness.run(|store| {
        store.execute_cooperative(host, key(), || panic!("no recursion"), || build(store))
    })
}

fn warm_read(harness: &GraphFixture, host: &VerterHost) -> CacheRead<QueryResult<SemanticNodeId>> {
    harness.run(|store| {
        store.execute_cooperative(
            host,
            key(),
            || panic!("no recursion"),
            || -> QueryBuildOutput<SemanticNodeId> { panic!("a warm read does not rebuild") },
        )
    })
}

fn registrations(harness: &GraphFixture) -> usize {
    harness.run(|store| store.shared_allocation_registrations_for_tests())
}

/// Pinned bytes beyond the store's own storage pins: what callers hold —
/// the allocations live deliveries hold and their metadata.
fn caller_pinned(harness: &GraphFixture, account: &SemanticRetentionAccount) -> usize {
    account.snapshot().pinned_bytes - harness.run(|store| store.store_storage_pins_for_tests())
}

/// The bytes a delivery's own metadata is pinned for.
fn metadata<T>(read: &CacheRead<T>) -> usize {
    read.lease().own_charge_bytes_for_tests()
}

/// Cache first: the candidate drains while a warm read still holds the
/// diagnostics. Exactly the rail stays charged, now pinned to that read —
/// the candidate's own bookkeeping and its carrier are released with it —
/// beside the read's own delivery metadata, and the read's drop restores
/// the account.
#[test]
fn a_read_outliving_its_candidate_keeps_exactly_the_delivered_rail_charged() {
    let host = host();
    let account = unbounded();
    let harness = GraphFixture::with_account(StoreAccount::new(Arc::clone(&account)));
    let cold = read(&harness, &host);
    assert_eq!(harness.run(|store| store.memo_entry_count()), 1);
    let warm = warm_read(&harness, &host);
    let retained = account.snapshot();
    assert!(
        retained.retained_bytes >= rail_charge(),
        "the retained candidate is charged for the diagnostic text it delivers: {retained:?}"
    );
    assert!(
        metadata(&cold) > 0 && metadata(&warm) > 0,
        "each delivery owner pins its own metadata"
    );
    assert_eq!(
        caller_pinned(&harness, &account),
        metadata(&cold),
        "while the candidate retains the rail and its own delivery — which the warm read \
         shares — callers pin only the cold winner's delivery metadata"
    );
    drop(cold);

    harness.run(|store| store.invalidate_all());
    assert_eq!(harness.run(|store| store.memo_entry_count()), 0);
    let held: RetentionAccountSnapshot = account.snapshot();
    assert_eq!(
        caller_pinned(&harness, &account),
        rail_charge() + metadata(&warm),
        "the delivered rail moves to the read that holds it, whole and once, beside the \
         read's own metadata; nothing the cache alone kept stays charged"
    );
    assert_eq!(
        registrations(&harness),
        0,
        "the held rail and its names carry their own charges: nothing stays registered"
    );
    assert!(
        held.retained_bytes + caller_pinned(&harness, &account) < retained.retained_bytes,
        "the candidate's bookkeeping and carrier were released with it"
    );

    drop(warm);
    assert_eq!(account.snapshot().retained_bytes, 0);
    assert_eq!(caller_pinned(&harness, &account), 0);
    assert_eq!(registrations(&harness), 0);
    assert_eq!(harness.live_nodes(), 0);
    drop(harness);
    assert_eq!(
        account.snapshot().total_bytes(),
        0,
        "the isolated owner's teardown restores the baseline"
    );
}

/// Caller first: reads drop while the candidate lives, which keeps the
/// one retained charge; the candidate's drain then releases everything.
#[test]
fn reads_dropped_first_leave_one_retained_charge_until_the_candidate_drains() {
    let host = host();
    let account = unbounded();
    let harness = GraphFixture::with_account(StoreAccount::new(Arc::clone(&account)));
    let cold = read(&harness, &host);
    let warm = warm_read(&harness, &host);
    let warm_again = warm.clone();
    let retained = account.snapshot();
    let candidate_metadata = metadata(&warm);

    drop((cold, warm, warm_again));
    assert_eq!(
        account.snapshot().retained_bytes,
        retained.retained_bytes,
        "a reader's drop changes nothing the candidate retains"
    );
    assert!(candidate_metadata > 0);
    assert_eq!(
        caller_pinned(&harness, &account),
        0,
        "the cold winner's metadata is released with it; the warm reads shared the \
         candidate's delivery, whose metadata the candidate retains"
    );

    harness.run(|store| store.invalidate_all());
    let drained = account.snapshot();
    assert_eq!(drained.retained_bytes, 0, "nothing retained: {drained:?}");
    assert_eq!(caller_pinned(&harness, &account), 0);
    assert_eq!(registrations(&harness), 0);
    drop(harness);
    assert_eq!(account.snapshot().total_bytes(), 0);
}

/// Zero headroom: the complete value is returned uncached, and the caller
/// still owns its value — charged as the caller's, pinned, for exactly as
/// long as the delivery holds it, never refused and never retained.
#[test]
fn a_refused_admission_returns_the_complete_value_charged_to_its_caller() {
    let host = host();
    let account = SemanticRetentionAccount::new(RetentionLimits {
        aggregate_ceiling_bytes: 0,
        ..RetentionLimits::defaults()
    });
    let harness = GraphFixture::with_account(StoreAccount::new(Arc::clone(&account)));
    let delivered = read(&harness, &host);
    assert!(matches!(delivered.value(), QueryResult::Value(_)));
    assert!(!delivered.result_is_partial());
    assert_eq!(delivered.walker_diagnostics().len(), 1);
    assert_eq!(harness.run(|store| store.memo_entry_count()), 0);
    assert_eq!(account.snapshot().retained_bytes, 0);
    assert_eq!(
        caller_pinned(&harness, &account),
        rail_charge() + metadata(&delivered),
        "the delivered rail, oversized name included, and the delivery's metadata are \
         the caller's, charged despite the refusal"
    );
    assert_eq!(
        registrations(&harness),
        0,
        "the rail and its two names carry their own charges"
    );
    let clone = delivered.clone();
    drop(delivered);
    assert_eq!(
        caller_pinned(&harness, &account),
        rail_charge() + metadata(&clone),
        "a clone of the read holds the same charges"
    );
    drop(clone);
    assert_eq!(caller_pinned(&harness, &account), 0);
    assert_eq!(registrations(&harness), 0);
    assert_eq!(harness.live_nodes(), 0);
}

/// An owning clone of a delivered name — a leaf of the diagnostic list —
/// that outlives the read, the cache and the whole store keeps exactly that
/// name charged, pinned, and its own drop restores the baseline at once:
/// no sweep, no reap. Holds in both release orders and with no headroom at
/// all, where the read was refused admission.
#[test]
fn a_cloned_delivered_name_alone_keeps_its_charge_until_its_own_drop() {
    for (ceiling, read_first) in [(usize::MAX / 2, true), (usize::MAX / 2, false), (0, true)] {
        let host = host();
        let account = SemanticRetentionAccount::new(RetentionLimits {
            aggregate_ceiling_bytes: ceiling,
            active_ceiling_bytes: ceiling,
            max_entry_bytes: ceiling,
            pin_threshold_bytes: ceiling,
        });
        let harness = GraphFixture::with_account(StoreAccount::new(Arc::clone(&account)));
        let read = read(&harness, &host);
        let cached = harness.run(|store| store.memo_entry_count()) == 1;
        assert_eq!(
            cached,
            ceiling > 0,
            "admitted exactly when there is headroom"
        );
        let name = delivered_name(&read);
        assert_eq!(
            name.charge_bytes(),
            Some(OwnedStr::charge_for_len(NAME_LEN))
        );

        if read_first {
            drop(read);
            harness.run(|store| store.invalidate_all());
        } else {
            harness.run(|store| store.invalidate_all());
            drop(read);
        }
        drop(harness);
        assert_eq!(
            account.snapshot().retained_bytes,
            0,
            "no cache retains anything once its store is gone"
        );
        assert_eq!(
            account.snapshot().pinned_bytes,
            OwnedStr::charge_for_len(NAME_LEN),
            "the clone alone keeps its name charged, exactly once"
        );
        assert_eq!(name.charge_class(), Some(ChargeClass::Pinned));
        drop(name);
        assert_eq!(
            account.snapshot().total_bytes(),
            0,
            "the final holder's drop restores the baseline"
        );
    }
}

/// An owning clone of a delivered dependency rail keeps the rail and every
/// canonical it names charged after the read and the store are gone; a
/// clone of one entry then keeps that canonical alone, and its drop
/// restores the baseline.
#[test]
fn a_cloned_dependency_rail_and_entry_keep_their_charges_through_their_last_holder() {
    use verter_type_engine::semantic_query::{DepEntry, DepSignature, DepVersion};
    const CANONICAL_LEN: usize = 12_000;
    let host = host();
    let account = unbounded();
    let harness = GraphFixture::with_account(StoreAccount::new(Arc::clone(&account)));
    let long = format!("/{}.ts", "d".repeat(CANONICAL_LEN));
    let read = harness.run(|store| {
        store.execute_cooperative(
            &host,
            key(),
            || panic!("no recursion"),
            || {
                let mut output = build(store);
                output.walker_diagnostics.clear();
                output.dep_signature = DepSignature::new(vec![
                    DepEntry::new(&long, DepVersion::ProjectGeneration(0)),
                    DepEntry::new("/short.ts", DepVersion::ProjectGeneration(0)),
                ]);
                output
            },
        )
    });
    let rail = read.dep_signature().clone();
    assert_eq!(rail.len(), 2);
    let entry = rail[0].clone();
    let rail_bytes = rail.charge_bytes().expect("the delivery charged its rail");
    assert_eq!(
        rail_bytes,
        Owned::<verter_type_engine::semantic_query::DepRail>::cell_block_bytes()
            + heap_block_bytes(2 * std::mem::size_of::<DepEntry>()),
        "the rail pays its own cell and entry backing; each canonical pays its own"
    );
    let names = OwnedStr::charge_for_len(long.len()) + OwnedStr::charge_for_len("/short.ts".len());

    drop(read);
    harness.run(|store| store.invalidate_all());
    drop(harness);
    assert_eq!(
        account.snapshot().pinned_bytes,
        rail_bytes + names,
        "the rail clone keeps the rail and both canonicals charged, once each"
    );
    drop(rail);
    assert_eq!(
        account.snapshot().pinned_bytes,
        OwnedStr::charge_for_len(long.len()),
        "the entry clone keeps its own canonical, and nothing else"
    );
    drop(entry);
    assert_eq!(account.snapshot().total_bytes(), 0);
}

/// A substitution answer is charged for as long as the memo keeps it and
/// released with it: the substitution memo is no uncharged store.
#[test]
fn a_substitution_answer_is_charged_while_its_memo_keeps_it() {
    let account = unbounded();
    let harness = GraphFixture::with_account(StoreAccount::new(Arc::clone(&account)));
    harness.run(|store| {
        let [a, b, c] = [
            PrimitiveKind::String,
            PrimitiveKind::Number,
            PrimitiveKind::Boolean,
        ]
        .map(|kind| store.intern_node(SemanticNodeData::Primitive(kind)));
        store.substitute_memo_publish(a, b, c, a);
        assert!(store.substitute_memo_get(a, b, c).is_some());
    });
    assert!(account.snapshot().retained_bytes > 0);
    harness.run(|store| store.invalidate_all());
    assert_eq!(account.snapshot().retained_bytes, 0);
    drop(harness);
    assert_eq!(account.snapshot().total_bytes(), 0);
}

/// A sealed refusal — a failed evaluation's summary — charges the rail its
/// read delivers by the rail's size, and once the table drops the summary a
/// read still holding that rail keeps it charged until it drops.
#[test]
fn a_sealed_refusal_hands_its_delivered_rail_to_a_surviving_read() {
    use verter_type_engine::project_semantic_dispatch::cost_receipt::{
        BudgetProfile, BudgetProfileSpec, COST_MODEL_REVISION,
    };
    let host = host();
    let account = unbounded();
    let harness = GraphFixture::with_account(StoreAccount::new(Arc::clone(&account)));
    let read = harness.run(|store| {
        store.execute_cooperative(
            &host,
            key(),
            || panic!("no recursion"),
            || {
                let mut output = build(store);
                output.cache_suppress = true;
                output
            },
        )
    });
    assert_eq!(harness.run(|store| store.memo_entry_count()), 0);
    assert_eq!(account.snapshot().retained_bytes, 0);
    let read = read.map_value(|result| match result {
        QueryResult::Value(node) => QueryResult::Value(SemanticQueryValue::TypeNode(node)),
        _ => panic!("the build answered a value"),
    });
    let held = read.clone();
    let profile = BudgetProfile::intern(BudgetProfileSpec {
        work: 1,
        bytes: 1024,
        query_depth: 2,
        instantiation_depth: 2,
        tail_steps: 2,
        request_operations: 0,
        relation_comparisons: 2,
        cost_model_revision: COST_MODEL_REVISION,
    });
    harness.run(|store| {
        assert!(store.seal_refusal_for_tests(
            &host,
            key(),
            profile,
            store.refusal_entry_for_tests(&host),
            read,
            verter_session_query::facts::fact_cache::ReadSetSignature::new(Arc::from([])),
        ));
    });
    let sealed = account.snapshot();
    assert!(
        sealed.retained_bytes >= rail_charge(),
        "the summary is charged for the diagnostic text its read keeps: {sealed:?}"
    );

    harness.run(|store| store.invalidate_all());
    assert_eq!(
        caller_pinned(&harness, &account),
        rail_charge() + metadata(&held),
        "the held read keeps exactly the rail charged once the summary is gone"
    );
    drop(held);
    assert_eq!(account.snapshot().retained_bytes, 0);
    assert_eq!(caller_pinned(&harness, &account), 0);
    assert_eq!(registrations(&harness), 0);
    drop(harness);
    assert_eq!(account.snapshot().total_bytes(), 0);
}

/// A flight's cell is live work: pinned from its claim, and released when
/// the aborted flight's last holder drops it.
#[test]
fn an_aborted_flight_releases_its_storage_pin() {
    let account = unbounded();
    let harness = GraphFixture::with_account(StoreAccount::new(Arc::clone(&account)));
    let before = caller_pinned(&harness, &account);
    harness.run(|store| {
        let flight = store
            .begin_inline_member_flight_for_tests(key())
            .expect("no other producer owns the key");
        let account_pinned = account.snapshot().pinned_bytes - store.graph_storage_charge_bytes();
        assert!(account_pinned > before, "the open flight's cell is charged");
        store.abort_inline_member_flight(&flight);
        drop(flight);
    });
    assert_eq!(
        caller_pinned(&harness, &account),
        before,
        "the aborted flight's cell is released with its last holder"
    );
}

/// A member whose family was keyed when the batch staged it, then evicted
/// by the family budget before the batch's lock while another holder spent
/// the headroom, must still be keyed under the lock: the batch publishes
/// every member or none, never a component missing the member whose keying
/// was refused.
#[test]
fn an_scc_batch_never_publishes_without_a_member_whose_family_was_evicted_meanwhile() {
    use verter_session_query::facts::fact_cache::{FactVersionRef, ReadSetSignature};
    use verter_type_engine::semantic_query::{RelateMemoKey, RelationContext, RelationOutcome};
    use verter_type_engine::semantic_query_memo::{
        MemoFamilyBound, PendingRelationMember, SccRootWitness,
    };

    const CEILING: usize = 64 * 1024 * 1024;
    let account = SemanticRetentionAccount::new(RetentionLimits {
        aggregate_ceiling_bytes: CEILING,
        active_ceiling_bytes: CEILING,
        max_entry_bytes: CEILING,
        pin_threshold_bytes: CEILING,
    });
    // Three resident families at most: keying two more evicts the oldest.
    let store = SemanticGraphStore::with_account(StoreAccount::new(Arc::clone(&account)))
        .with_memo_family_bound(MemoFamilyBound::ForTest(3));
    let carrier = ReadSetSignature::new(Arc::from(vec![FactVersionRef::FileWholeHash {
        canonical_id: "/w/scc.ts".to_owned(),
        hash: [5; 16],
    }]));
    let roots: Arc<[Arc<str>]> = Arc::from(vec![Arc::<str>::from("/w/scc.ts")]);
    let target = store.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Unknown));
    let key_for = |kind| {
        RelateMemoKey::assignable(
            store.intern_node(SemanticNodeData::Primitive(kind)),
            target,
            RelationContext::default(),
        )
    };
    let seed = |key: &RelateMemoKey, outcome| {
        store.insert_relation_payload_for_tests(
            key.clone(),
            carrier.clone(),
            Arc::clone(&roots),
            store.relation_payload_for_tests(outcome),
            0,
        );
    };
    // The member's family is keyed first (oldest), the root's second.
    let member_key = key_for(PrimitiveKind::Number);
    let root_key = key_for(PrimitiveKind::String);
    seed(&member_key, RelationOutcome::NotAssignable);
    seed(&root_key, RelationOutcome::Assignable);
    let root_seq = store
        .relation_published_carrier(&root_key)
        .expect("published root")
        .admission_seq;
    let flight = store
        .begin_inline_relation_flight(&member_key)
        .expect("member flight");
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let _gate = store.test_relation_member_pre_entries_gate(Arc::clone(&barrier));
    let published = std::thread::scope(|scope| {
        let publisher = scope.spawn(|| {
            store.publish_scc_members_fenced_for_tests(
                None::<
                    &dyn verter_type_engine::resolver_core::ResolverContext<
                        crate::resolver_core::HostCapabilities,
                    >,
                >,
                &SccRootWitness::relate(root_key.clone(), root_seq),
                &carrier,
                &roots,
                0,
                vec![PendingRelationMember {
                    key: member_key.clone(),
                    payload: store.relation_payload_for_tests(RelationOutcome::Assignable),
                    flight,
                }],
                Vec::new(),
                Vec::new(),
            )
        });
        barrier.wait();
        // Inside the window: two more families key in, so the budget evicts
        // the oldest — the member's — and a live handle elsewhere spends
        // all the headroom.
        seed(
            &key_for(PrimitiveKind::Boolean),
            RelationOutcome::Assignable,
        );
        seed(&key_for(PrimitiveKind::BigInt), RelationOutcome::Assignable);
        assert!(
            store.relation_published_carrier(&member_key).is_none(),
            "the member's family was evicted"
        );
        assert!(store.relation_published_carrier(&root_key).is_some());
        let pressure = account.pin(CEILING);
        barrier.wait();
        let published = publisher.join().expect("publisher");
        drop(pressure);
        published
    });
    assert!(
        !published || store.relation_published_carrier(&member_key).is_some(),
        "a batch that reports publication published every member"
    );
}
