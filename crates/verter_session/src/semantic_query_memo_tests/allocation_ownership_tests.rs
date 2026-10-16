//! Retained memo allocations are charged by what they actually allocate,
//! once, and a delivery that outlives the cache keeps the allocations it
//! aliases charged — pinned to its caller — until its final holder drops.
//! The cache's own bookkeeping never outlives the cache.

use std::sync::Arc;

use super::*;
use crate::{HostConfig, UpsertRequest, VerterHost};
use verter_session_query::retention::allocation::arc_block_bytes;
use verter_session_query::retention::shared::registration_bytes;
use verter_session_query::retention::{
    RetentionAccountSnapshot, RetentionLimits, SemanticRetentionAccount, StoreAccount,
};
use verter_type_engine::project_semantic_dispatch::walk::{QueryBuildOutput, ShallowDiagnostic};
use verter_type_engine::semantic_query::{
    CacheRead, LiteralValue, PrimitiveKind, ResolveDeclKey, ScopeId,
};
use verter_type_engine::semantic_query_memo::ownership_harness::CountedOwnershipHarness;
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
        name: Arc::from("n".repeat(NAME_LEN)),
        owner_canonical: Arc::from(OWNER),
        owner: TopLevelOwnerId::ordinary_file(),
    }];
    output
}

/// What the delivered rail costs, from its allocations alone: the slice
/// block holding the one diagnostic, the two texts it names, and its one
/// registration. A charge missing the name, or a second registration of the
/// rail, misses this figure.
fn rail_charge() -> usize {
    arc_block_bytes(std::mem::size_of::<ShallowDiagnostic>())
        + arc_block_bytes(NAME_LEN)
        + arc_block_bytes(OWNER.len())
        + registration_bytes()
}

fn read(
    harness: &CountedOwnershipHarness,
    host: &VerterHost,
) -> CacheRead<QueryResult<SemanticNodeId>> {
    harness.run(|store| {
        store.execute_cooperative(host, key(), || panic!("no recursion"), || build(store))
    })
}

fn warm_read(
    harness: &CountedOwnershipHarness,
    host: &VerterHost,
) -> CacheRead<QueryResult<SemanticNodeId>> {
    harness.run(|store| {
        store.execute_cooperative(
            host,
            key(),
            || panic!("no recursion"),
            || -> QueryBuildOutput<SemanticNodeId> { panic!("a warm read does not rebuild") },
        )
    })
}

fn registrations(harness: &CountedOwnershipHarness) -> usize {
    harness.run(|store| store.shared_allocation_registrations_for_tests())
}

/// Pinned bytes beyond the graph's own storage pin: what callers hold.
fn caller_pinned(harness: &CountedOwnershipHarness, account: &SemanticRetentionAccount) -> usize {
    account.snapshot().pinned_bytes - harness.run(|store| store.graph_storage_charge_bytes())
}

/// Cache first: the candidate drains while a warm read still holds the
/// diagnostics. Exactly the rail stays charged, now pinned to that read —
/// the candidate's own bookkeeping and its carrier are released with it —
/// and the read's drop restores the account.
#[test]
fn a_read_outliving_its_candidate_keeps_exactly_the_delivered_rail_charged() {
    let host = host();
    let account = unbounded();
    let harness = CountedOwnershipHarness::with_account(StoreAccount::new(Arc::clone(&account)));
    let cold = read(&harness, &host);
    assert_eq!(harness.run(|store| store.memo_entry_count()), 1);
    let warm = warm_read(&harness, &host);
    let retained = account.snapshot();
    assert!(
        retained.retained_bytes >= rail_charge(),
        "the retained candidate is charged for the diagnostic text it delivers: {retained:?}"
    );
    assert_eq!(caller_pinned(&harness, &account), 0);
    drop(cold);

    harness.run(|store| store.invalidate_all());
    assert_eq!(harness.run(|store| store.memo_entry_count()), 0);
    let held: RetentionAccountSnapshot = account.snapshot();
    assert_eq!(
        caller_pinned(&harness, &account),
        rail_charge(),
        "the delivered rail moves to the read that holds it, whole and once; nothing \
         the cache alone kept stays charged"
    );
    assert_eq!(
        registrations(&harness),
        1,
        "only the held rail stays registered"
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
    let harness = CountedOwnershipHarness::with_account(StoreAccount::new(Arc::clone(&account)));
    let cold = read(&harness, &host);
    let warm = warm_read(&harness, &host);
    let warm_again = warm.clone();
    let retained = account.snapshot();

    drop((cold, warm, warm_again));
    assert_eq!(
        account.snapshot(),
        RetentionAccountSnapshot {
            releases: account.snapshot().releases,
            ..retained
        },
        "a reader's drop changes nothing while the candidate retains the allocation"
    );

    harness.run(|store| store.invalidate_all());
    let drained = account.snapshot();
    assert_eq!(drained.retained_bytes, 0, "nothing retained: {drained:?}");
    assert_eq!(caller_pinned(&harness, &account), 0);
    assert_eq!(registrations(&harness), 0);
    drop(harness);
    assert_eq!(account.snapshot().total_bytes(), 0);
}

/// Zero headroom: the complete value is returned uncached, nothing is
/// charged and nothing is registered, and the caller still owns its value.
#[test]
fn a_refused_admission_returns_the_complete_value_with_nothing_charged() {
    let host = host();
    let account = SemanticRetentionAccount::new(RetentionLimits {
        aggregate_ceiling_bytes: 0,
        ..RetentionLimits::defaults()
    });
    let harness = CountedOwnershipHarness::with_account(StoreAccount::new(Arc::clone(&account)));
    let delivered = read(&harness, &host);
    assert!(matches!(delivered.value(), QueryResult::Value(_)));
    assert!(!delivered.result_is_partial());
    assert_eq!(delivered.walker_diagnostics().len(), 1);
    assert_eq!(harness.run(|store| store.memo_entry_count()), 0);
    assert_eq!(account.snapshot().retained_bytes, 0);
    assert_eq!(
        caller_pinned(&harness, &account),
        0,
        "only the graph's storage is pinned"
    );
    assert_eq!(registrations(&harness), 0);
    drop(delivered);
    assert_eq!(harness.live_nodes(), 0);
}

/// A substitution answer is charged for as long as the memo keeps it and
/// released with it: the substitution memo is no uncharged store.
#[test]
fn a_substitution_answer_is_charged_while_its_memo_keeps_it() {
    let account = unbounded();
    let harness = CountedOwnershipHarness::with_account(StoreAccount::new(Arc::clone(&account)));
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
    let harness = CountedOwnershipHarness::with_account(StoreAccount::new(Arc::clone(&account)));
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
        rail_charge(),
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
    let harness = CountedOwnershipHarness::with_account(StoreAccount::new(Arc::clone(&account)));
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
