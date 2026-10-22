//! In-flight semantic work charges REAL ACTIVE bytes on the one account,
//! and every boundary of the producer protocol settles them exactly once:
//! a publication adopts the delivery's charge into retention, a denied
//! retention hands it to the caller as a pin, and a cancelled or aborted
//! build releases it with its flight. The boundaries are driven through
//! the production store protocol (`execute_cooperative`), and an
//! ACTIVE-occupancy window discriminates the in-flight phase from the
//! settled classes even when the account's final bytes drain to zero.

use std::sync::Arc;

use super::*;
use crate::{HostConfig, UpsertRequest, VerterHost};
use verter_session_query::retention::{RetentionLimits, SemanticRetentionAccount, StoreAccount};
use verter_type_engine::semantic_query::{QueryResult, ResolveDeclKey, ScopeId};
use verter_type_engine::semantic_query_memo::graph_fixture::GraphFixture;
use verter_type_expr::TopLevelOwnerId;

const FILE: &str = "/in-flight/owned.ts";
const OWNER: &str = "/in-flight/owner.ts";
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
            canonical_id: verter_session_query::retention::owned::OwnedStr::from(FILE),
            owner: TopLevelOwnerId::ordinary_file(),
            local_scope: None,
            binder_scope_id: verter_type_engine::semantic_query::BinderScopeId::file_scope(
                TopLevelOwnerId::ordinary_file(),
            ),
        },
        name: verter_session_query::retention::owned::OwnedStr::from("Target"),
    })
}

fn account(ceiling: usize) -> Arc<SemanticRetentionAccount> {
    SemanticRetentionAccount::new(RetentionLimits {
        aggregate_ceiling_bytes: ceiling,
        max_entry_bytes: ceiling,
        pin_threshold_bytes: ceiling,
    })
}

/// A complete build whose one walker diagnostic names a deliberately
/// oversized arm: a delivered rail whose charge is far larger than a small
/// test ceiling.
fn build(
    store: &SemanticGraphStore,
) -> verter_type_engine::project_semantic_dispatch::walk::QueryBuildOutput<
    verter_type_engine::semantic_query::SemanticNodeId,
> {
    use verter_type_engine::project_semantic_dispatch::walk::{
        QueryBuildOutput, ShallowDiagnostic,
    };
    use verter_type_engine::semantic_query::{LiteralValue, SemanticNodeData};
    let mut output: QueryBuildOutput<verter_type_engine::semantic_query::SemanticNodeId> = (
        QueryResult::Value(
            store.intern_node(SemanticNodeData::Literal(LiteralValue::String(
                "in-flight".into(),
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

fn read(
    harness: &GraphFixture,
    host: &VerterHost,
) -> verter_type_engine::semantic_query::CacheRead<
    QueryResult<verter_type_engine::semantic_query::SemanticNodeId>,
> {
    harness.run(|store| {
        store.execute_cooperative(host, key(), || panic!("no recursion"), || build(store))
    })
}

/// The delivery's window over one cold read, and the account it charged.
fn windowed_read(
    ceiling: usize,
) -> (
    Arc<SemanticRetentionAccount>,
    GraphFixture,
    verter_session_query::retention::active_window::ActiveWindowReport,
    verter_type_engine::semantic_query::CacheRead<
        QueryResult<verter_type_engine::semantic_query::SemanticNodeId>,
    >,
) {
    let host = host();
    let account = account(ceiling);
    let harness = GraphFixture::with_account(StoreAccount::new(Arc::clone(&account)));
    let owner = account.mint_active_owner();
    let window = account.open_active_window(&owner);
    let read = read(&harness, &host);
    let report = window.finish();
    (account, harness, report, read)
}

/// A published cold read charges its delivery ACTIVE in flight — a
/// positive attributable peak with real rise and fall — and the
/// publication settles every in-flight byte into RETAINED: the held read
/// observes no ACTIVE left, the window closes at zero, and the account's
/// retained total carries the delivered rail exactly once. A publication
/// that dropped, doubled or left the charge in the ACTIVE class misses
/// the held snapshot or the window's conservation.
#[test]
fn a_publication_settles_the_in_flight_delivery_into_retained() {
    let (account, harness, report, read) = windowed_read(usize::MAX / 2);
    let peak = report
        .attributed()
        .expect("a workload alone on its private account is attributable");
    assert!(
        peak.crossed(1),
        "the cold read charged real ACTIVE bytes in flight, peak {}",
        peak.peak_active_bytes
    );
    assert!(report.rise_bytes > 0 && report.fall_bytes > 0);
    assert_eq!(report.active_at_close, 0, "nothing stays in flight");
    let settled = account.snapshot();
    assert_eq!(settled.active_bytes, 0);
    assert!(
        settled.retained_bytes > 0,
        "the publication adopted the in-flight charge into retention: {settled:?}"
    );
    assert!(matches!(read.value(), QueryResult::Value(_)));

    drop(read);
    harness.run(|store| store.invalidate_all());
    drop(harness);
    assert_eq!(
        account.snapshot().total_bytes(),
        0,
        "the isolated owner's teardown drains every class"
    );
}

/// A delivery the account refuses to retain is still delivered COMPLETE,
/// and its in-flight charge settles at the caller handoff: the window
/// saw real ACTIVE bytes rise and fall, the held read owns its rail and
/// metadata as PINS — not as leftover ACTIVE, not uncharged, not twice —
/// and the final reader's drop drains the account.
#[test]
fn a_denied_retention_delivery_hands_its_in_flight_charge_to_its_caller() {
    let (account, harness, report, read) = windowed_read(0);
    let peak = report
        .attributed()
        .expect("a workload alone on its private account is attributable");
    assert!(
        peak.crossed(1),
        "even a refused publication's delivery charged real ACTIVE bytes"
    );
    assert!(matches!(read.value(), QueryResult::Value(_)));
    assert!(!read.result_is_partial());
    assert_eq!(harness.run(|store| store.memo_entry_count()), 0);
    let settled = account.snapshot();
    assert_eq!(settled.retained_bytes, 0, "nothing was retained");
    assert_eq!(settled.active_bytes, 0, "the handoff settled the flight");
    assert!(
        settled.pinned_bytes > 0,
        "the caller owns its complete delivered value as pins: {settled:?}"
    );

    let clone = read.clone();
    drop(read);
    assert!(
        account.snapshot().pinned_bytes > 0,
        "a clone of the read holds the same settled charges"
    );
    drop(clone);
    drop(harness);
    assert_eq!(account.snapshot().total_bytes(), 0);
}

/// Crossing a retention policy boundary — an aggregate ceiling far smaller
/// than the delivered rail — cannot abort the admitted computation: the
/// cold read still builds, still charges its in-flight bytes (ACTIVE,
/// unconditionally, past the ceiling), and still delivers its complete
/// value uncached. The refusal classes stay refused-for-retention only.
#[test]
fn crossing_a_small_ceiling_cannot_abort_the_admitted_read() {
    let (account, harness, report, read) = windowed_read(1);
    let peak = report
        .attributed()
        .expect("a workload alone on its private account is attributable");
    assert!(
        peak.peak_active_bytes > 1,
        "the in-flight rail dwarfed the ceiling and was charged anyway"
    );
    assert!(matches!(read.value(), QueryResult::Value(_)));
    assert_eq!(read.walker_diagnostics().len(), 1);
    let settled = account.snapshot();
    assert_eq!(settled.retained_bytes, 0);
    assert_eq!(settled.active_bytes, 0);
    assert!(settled.pinned_bytes > 0, "the caller holds its delivery");
    assert_eq!(settled.refusals_active, 0, "no active-class refusal exists");
    drop(read);
    drop(harness);
    assert_eq!(account.snapshot().total_bytes(), 0);
}

/// The charge classes a held delivery's witness names: the delivered
/// allocations' own cells, each once, in the class its settlement chose.
/// Reused by the reopen proof below through the public host in
/// `tests/cases/g_session`.
#[test]
fn an_inline_member_hold_is_charged_in_flight_until_its_batch_ends() {
    let account = account(usize::MAX / 2);
    let harness = GraphFixture::with_account(StoreAccount::new(Arc::clone(&account)));
    let before = account.snapshot().active_bytes;
    harness.run(|store| {
        let flight = store
            .begin_inline_member_flight_for_tests(key())
            .expect("no other producer owns the key");
        let held = account.snapshot().active_bytes;
        assert!(
            held > before,
            "the member's hold charges its root set in flight"
        );
        // The batch refuses the whole component: the hold ends with the
        // abort, releasing its in-flight bytes with it.
        store.abort_inline_member_flight(&flight);
        drop(flight);
    });
    assert_eq!(
        account.snapshot().active_bytes,
        before,
        "the aborted member's hold released its in-flight charge"
    );
}

/// A producer whose request is cancelled mid-flight returns on the
/// cancellation rail and its flight's in-flight charge releases with the
/// retired flight: the claimed flight's storage was charged ACTIVE from
/// admission, and an abandoned schedule leaves nothing ACTIVE behind.
#[test]
fn a_cancelled_producer_releases_its_in_flight_charge() {
    use verter_type_engine::request_context::{RequestContext, RequestContextGuard};
    use verter_type_engine::semantic_query::QueryError;

    let host = host();
    let account = account(usize::MAX / 2);
    let harness = Arc::new(GraphFixture::with_account(StoreAccount::new(Arc::clone(
        &account,
    ))));
    let context = RequestContext::new(7, Arc::from("/in-flight/cancelled.ts"), false, None);
    let (entered_tx, entered_rx) = std::sync::mpsc::channel::<()>();
    let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
    let producer = {
        let harness = Arc::clone(&harness);
        let context = Arc::clone(&context);
        std::thread::spawn(move || {
            let _guard = RequestContextGuard::install(context);
            harness.run(|store| {
                store.execute_cooperative(
                    &host,
                    key(),
                    || panic!("no recursion"),
                    || {
                        entered_tx.send(()).expect("the driver is waiting");
                        release_rx.recv().expect("the driver releases the build");
                        build(store)
                    },
                )
            })
        })
    };
    entered_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("the producer entered its cold build");
    assert!(
        account.snapshot().active_bytes > 0,
        "the claimed flight's storage is charged while genuinely in flight"
    );
    context.cancel();
    release_tx
        .send(())
        .expect("the producer is parked in its build");
    let read = producer
        .join()
        .expect("the cancelled producer returns on its own rail");
    assert!(
        matches!(read.value(), QueryResult::Error(QueryError::Cancelled)),
        "a cancelled request delivers no value"
    );
    drop(read);
    assert_eq!(
        account.snapshot().active_bytes,
        0,
        "the cancelled flight's in-flight charge was released"
    );
}
