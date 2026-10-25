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
/// The delivered rail's size, chosen to sit ABOVE one full granule of the
/// arena's fresh-construction storage (64 KiB) so the delivery-payload
/// proofs below cannot be met by construction or flight storage alone,
/// and BELOW the fresh-payload rail: flight storage (a few KiB) <
/// construction granule (64 KiB) < NAME_LEN < PAYLOAD_LEN.
const NAME_LEN: usize = 100_000;
/// A fresh construction's payload rail: bytes of ONE literal node's heap,
/// far past the arena's storage granule, so a window that sees them cannot
/// be seeing flight storage or delivery metadata (a few KiB at most).
const PAYLOAD_LEN: usize = 300_000;

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
    named_key("Target")
}

fn named_key(name: &str) -> SemanticQueryKey {
    SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: ScopeId {
            canonical_id: verter_session_query::retention::owned::OwnedStr::from(FILE),
            owner: TopLevelOwnerId::ordinary_file(),
            local_scope: None,
            binder_scope_id: verter_type_engine::semantic_query::BinderScopeId::file_scope(
                TopLevelOwnerId::ordinary_file(),
            ),
        },
        name: verter_session_query::retention::owned::OwnedStr::from(name),
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
    build_with_payload(store, "in-flight")
}

/// [`build`]'s construction with its value's payload heap sized to
/// `payload`: a literal node whose own heap is the fresh construction's
/// dominant allocation. Two constructions of the SAME payload structurally
/// dedup to one node — the reuse the control proof below relies on.
fn build_with_payload(
    store: &SemanticGraphStore,
    payload: &str,
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
                payload.into(),
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
    read_build(harness, host, key(), "in-flight")
}

/// One cold read of `key` whose raw construction interns a literal node
/// with a `payload`-byte heap: the fresh-construction rail.
fn read_build(
    harness: &GraphFixture,
    host: &VerterHost,
    key: SemanticQueryKey,
    payload: &str,
) -> verter_type_engine::semantic_query::CacheRead<
    QueryResult<verter_type_engine::semantic_query::SemanticNodeId>,
> {
    harness.run(|store| {
        store.execute_cooperative(
            host,
            key,
            || panic!("no recursion"),
            || build_with_payload(store, payload),
        )
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
        peak.crossed(NAME_LEN),
        "the delivered rail itself charged ACTIVE in flight — flight storage \
         alone (a few KiB) cannot cross {NAME_LEN}: peak {}",
        peak.peak_active_bytes
    );
    assert!(
        report.rise_bytes >= NAME_LEN,
        "the payload rose inside the window, not only flight storage"
    );
    assert!(report.fall_bytes >= NAME_LEN);
    assert_eq!(report.active_at_close, 0, "nothing stays in flight");
    let settled = account.snapshot();
    assert_eq!(settled.active_bytes, 0);
    assert!(
        settled.retained_bytes >= NAME_LEN,
        "the publication adopted the delivered rail's charge into retention: {settled:?}"
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
        peak.crossed(NAME_LEN),
        "even a refused publication's delivery charged its rail ACTIVE in \
         flight — flight storage alone (a few KiB) cannot cross {NAME_LEN}: \
         peak {}",
        peak.peak_active_bytes
    );
    assert!(matches!(read.value(), QueryResult::Value(_)));
    assert!(!read.result_is_partial());
    assert_eq!(harness.run(|store| store.memo_entry_count()), 0);
    let settled = account.snapshot();
    assert_eq!(settled.retained_bytes, 0, "nothing was retained");
    assert_eq!(settled.active_bytes, 0, "the handoff settled the flight");
    assert!(
        settled.pinned_bytes >= NAME_LEN,
        "the caller owns its complete delivered rail as pins: {settled:?}"
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
        peak.crossed(NAME_LEN),
        "the delivered rail dwarfed the ceiling and was charged ACTIVE \
         anyway — flight storage alone (a few KiB) cannot cross {NAME_LEN}: \
         peak {}",
        peak.peak_active_bytes
    );
    assert!(matches!(read.value(), QueryResult::Value(_)));
    assert_eq!(read.walker_diagnostics().len(), 1);
    let settled = account.snapshot();
    assert_eq!(settled.retained_bytes, 0);
    assert_eq!(settled.active_bytes, 0);
    assert!(
        settled.pinned_bytes >= NAME_LEN,
        "the caller holds its delivered rail"
    );
    assert_eq!(settled.refusals_active, 0, "no active-class refusal exists");
    drop(read);
    drop(harness);
    assert_eq!(account.snapshot().total_bytes(), 0);
}

/// An inline member's flight holds the nodes of its decided value until
/// its batch ends. A value of more operands than a root set keeps inline is
/// held through the set's shared block, charged ACTIVE — in flight, never a
/// pin — for exactly the bytes the block occupies, shared by every clone of
/// the flight, and released with the counts it holds when the batch ends.
#[test]
fn an_inline_member_hold_is_charged_in_flight_until_its_batch_ends() {
    use verter_type_engine::semantic_query::{
        DeclarationAnalysisValue, LiteralValue, SemanticNodeData, SemanticQueryValue,
    };
    const OPERANDS: usize = 6;
    let account = account(usize::MAX / 2);
    let harness = GraphFixture::with_account(StoreAccount::new(Arc::clone(&account)));
    let before = account.snapshot().active_bytes;
    harness.run(|store| {
        let mut flight = store
            .begin_inline_member_flight_for_tests(key())
            .expect("no other producer owns the key");
        let after_flight = account.snapshot().active_bytes;
        assert!(
            after_flight > before,
            "the member's flight storage charges in flight"
        );
        // The construction scope stays open until the hold has rooted the
        // value, so its nodes are live for it.
        let scope = store.enter_node_root_scope();
        let nodes: Vec<_> = (0..OPERANDS)
            .map(|index| {
                store.intern_node(SemanticNodeData::Literal(LiteralValue::String(format!(
                    "member value {index}"
                ))))
            })
            .collect();
        let held_value = QueryResult::Value(SemanticQueryValue::DeclarationAnalysis(
            DeclarationAnalysisValue {
                contributors: nodes.clone().into(),
            },
        ));
        let counted = store.node_owner_count_for_tests(nodes[0]);
        let pinned_at_hold = account.snapshot().pinned_bytes;
        flight.hold_value_for_tests(store, &held_value);
        let after_hold = account.snapshot();
        let block =
            verter_type_engine::semantic_query_memo::node_roots::root_set_charge_bytes_for_tests(
                OPERANDS,
            );
        assert!(block > 0, "premise: the value's set needs a shared block");
        assert_eq!(
            after_hold.active_bytes - after_flight,
            block,
            "the hold charges its set's shared block in flight, exactly: {after_hold:?}"
        );
        assert_eq!(
            after_hold.pinned_bytes, pinned_at_hold,
            "the hold is in-flight work, not a pinned settlement"
        );
        assert_eq!(
            store.node_owner_count_for_tests(nodes[0]),
            counted + 1,
            "the hold counts every operand of the value"
        );
        // A clone of the flight shares the block, its counts and its charge.
        let replay = flight.clone();
        assert_eq!(account.snapshot().active_bytes, after_hold.active_bytes);
        assert_eq!(store.node_owner_count_for_tests(nodes[0]), counted + 1);
        drop(replay);
        // The batch refuses the whole component: the hold ends with the
        // abort, releasing its in-flight bytes and its counts with it.
        store.abort_inline_member_flight(&flight);
        drop(flight);
        assert_eq!(
            store.node_owner_count_for_tests(nodes[0]),
            counted,
            "the ended hold released its counts"
        );
        drop(scope);
    });
    assert_eq!(
        account.snapshot().active_bytes,
        before,
        "the aborted member's hold released its in-flight charge"
    );
}

/// An inline member's held set handed to a delivery is charged once: the
/// delivery adds only its owner's block, because the set's shared block is
/// already charged through its own cell; when the flight ends, the block's
/// in-flight charge passes to the delivery as a pin, and the delivery's
/// drop releases everything.
#[test]
fn an_inline_member_hold_shared_with_a_delivery_is_charged_once() {
    use verter_type_engine::semantic_query::{
        DeclarationAnalysisValue, LiteralValue, SemanticNodeData, SemanticQueryValue,
    };
    const OPERANDS: usize = 6;
    let account = account(usize::MAX / 2);
    let harness = GraphFixture::with_account(StoreAccount::new(Arc::clone(&account)));
    let total = || account.snapshot().total_bytes();
    let idle = account.snapshot().active_bytes;
    harness.run(|store| {
        let mut flight = store
            .begin_inline_member_flight_for_tests(key())
            .expect("no other producer owns the key");
        let scope = store.enter_node_root_scope();
        let nodes: Vec<_> = (0..OPERANDS)
            .map(|index| {
                store.intern_node(SemanticNodeData::Literal(LiteralValue::String(format!(
                    "shared member value {index}"
                ))))
            })
            .collect();
        let held_value = QueryResult::Value(SemanticQueryValue::DeclarationAnalysis(
            DeclarationAnalysisValue {
                contributors: nodes.clone().into(),
            },
        ));
        let block =
            verter_type_engine::semantic_query_memo::node_roots::root_set_charge_bytes_for_tests(
                OPERANDS,
            );
        let before_hold = account.snapshot();
        flight.hold_value_for_tests(store, &held_value);
        assert_eq!(
            account.snapshot().active_bytes - before_hold.active_bytes,
            block
        );

        // A delivery of one operand costs its owner's block alone.
        let base = total();
        let short = store.deliver_roots_for_tests(store.root_nodes([nodes[0]]).expect("live"));
        let owner_block = total() - base;
        drop(short);
        assert_eq!(total(), base);

        let shared = flight.held_roots_for_tests().expect("the value is held");
        let lease = store.deliver_roots_for_tests(shared);
        assert_eq!(
            total() - base,
            owner_block,
            "the delivery charges its owner's block; the shared block is charged once, in flight"
        );

        store.abort_inline_member_flight(&flight);
        drop(flight);
        let settled = account.snapshot();
        assert_eq!(
            settled.active_bytes, idle,
            "the ended flight and hold leave nothing in flight"
        );
        assert_eq!(
            settled.pinned_bytes - before_hold.pinned_bytes,
            owner_block + block,
            "the delivery keeps the shared block, charged once, pinned"
        );
        drop(lease);
        let released = account.snapshot();
        assert_eq!(
            (released.pinned_bytes, released.active_bytes),
            (before_hold.pinned_bytes, idle),
            "the delivery's drop releases its block and its own"
        );
        drop(scope);
    });
}

/// A cold construction's FRESH payload — the heap of the nodes its raw
/// build allocates, before any publication or caller handoff owns them —
/// charges ACTIVE on the same account: a window over the read sees a peak
/// and rise no flight storage or delivery metadata (a few KiB at most)
/// can explain, and the settle hands the construction to the arena's
/// settled storage so nothing stays ACTIVE. A construction charged as
/// pinned from its first allocation, or not charged at all, misses the
/// peak.
#[test]
fn a_cold_construction_charges_its_fresh_payload_active_until_it_settles() {
    let host = host();
    let account = account(usize::MAX / 2);
    let harness = GraphFixture::with_account(StoreAccount::new(Arc::clone(&account)));
    let payload = "p".repeat(PAYLOAD_LEN);
    let owner = account.mint_active_owner();
    let window = account.open_active_window(&owner);
    let read = read_build(&harness, &host, key(), &payload);
    let report = window.finish();
    let peak = report
        .attributed()
        .expect("a workload alone on its private account is attributable");
    assert!(
        peak.crossed(PAYLOAD_LEN),
        "the fresh construction's payload charged ACTIVE before \
         publication — flight storage and delivery metadata (a few KiB) \
         cannot cross {PAYLOAD_LEN}: peak {}",
        peak.peak_active_bytes
    );
    assert!(
        report.rise_bytes >= PAYLOAD_LEN,
        "the payload itself rose inside the window"
    );
    assert_eq!(report.active_at_close, 0, "the settle handed it over");
    assert!(matches!(read.value(), QueryResult::Value(_)));
    assert_eq!(
        account.snapshot().active_bytes,
        0,
        "nothing of the construction stays in flight after the delivery settles"
    );

    // Reused-node control: a SECOND cold read, on the same arena, whose
    // construction structurally reuses the held literal instead of
    // allocating it afresh, charges no fresh payload. Its window peak —
    // flight storage and delivery metadata only — cannot cross the
    // payload rail again, so the proof above measures FRESH construction,
    // not a blanket per-build charge.
    let second_owner = account.mint_active_owner();
    let second_window = account.open_active_window(&second_owner);
    let second = read_build(&harness, &host, named_key("Target2"), &payload);
    let second_report = second_window.finish();
    let second_peak = second_report
        .attributed()
        .expect("the second workload is attributable to its own owner");
    assert!(
        !second_peak.crossed(PAYLOAD_LEN),
        "a construction that reuses the live literal allocates no fresh \
         payload: peak {}",
        second_peak.peak_active_bytes
    );
    assert!(
        second_peak.crossed(1),
        "the second read's own flight is still real work"
    );
    drop((read, second));
    drop(harness);
    assert_eq!(account.snapshot().total_bytes(), 0);
}

/// A plain build output whose value is an already-interned node: no
/// diagnostics, so the construction's own storage is the only charge in
/// flight.
fn plain_output(
    node: verter_type_engine::semantic_query::SemanticNodeId,
) -> verter_type_engine::project_semantic_dispatch::walk::QueryBuildOutput<
    verter_type_engine::semantic_query::SemanticNodeId,
> {
    (QueryResult::Value(node), empty_signature()).into()
}

/// The bound a parked producer's coordination waits obey: a failing test
/// unwinds past its channels, so the producer's wait ends and its thread
/// joins instead of hanging the runner.
const HANG_BOUND: std::time::Duration = std::time::Duration::from_secs(20);

/// Releasing an OLDER node's storage while a construction is open pays the
/// freed allocation's OWN charge — the settled storage pin — never the open
/// construction's ACTIVE bytes: a still-live fresh payload stays ACTIVE
/// until ITS construction settles, whatever older storage is released
/// around it. A release that paid the open construction first would drain
/// ACTIVE to flight storage while the fresh payload is live.
#[test]
fn releasing_an_older_node_keeps_a_live_constructions_active_charge() {
    use verter_type_engine::semantic_query::{LiteralValue, SemanticNodeData};

    let host = host();
    let account = account(usize::MAX / 2);
    let harness = Arc::new(GraphFixture::with_account(StoreAccount::new(Arc::clone(
        &account,
    ))));
    let old_payload = "o".repeat(PAYLOAD_LEN);
    // An older node charged to the arena's settled storage, held past the
    // fixture scope by a lease on its payload: dropping the lease releases
    // the node's storage wherever it happens — here inside a fresh
    // construction's build.
    let held = harness.run(|store| {
        let node = store.intern_node(SemanticNodeData::Literal(LiteralValue::String(old_payload)));
        store
            .read_in_scope(node)
            .expect("the fresh node is live under the fixture scope")
    });
    let (allocated_tx, allocated_rx) = std::sync::mpsc::channel::<()>();
    let (drop_gate_tx, drop_gate_rx) = std::sync::mpsc::channel::<()>();
    let (dropped_tx, dropped_rx) = std::sync::mpsc::channel::<()>();
    let (finish_tx, finish_rx) = std::sync::mpsc::channel::<()>();
    let payload = "p".repeat(PAYLOAD_LEN);
    let producer = {
        let harness = Arc::clone(&harness);
        std::thread::spawn(move || {
            harness.run(|store| {
                store.execute_cooperative(
                    &host,
                    key(),
                    || panic!("no recursion"),
                    move || {
                        let fresh = store
                            .intern_node(SemanticNodeData::Literal(LiteralValue::String(payload)));
                        allocated_tx.send(()).expect("the driver waits");
                        drop_gate_rx
                            .recv_timeout(HANG_BOUND)
                            .expect("the driver releases the older node");
                        drop(held);
                        dropped_tx.send(()).expect("the driver waits");
                        finish_rx
                            .recv_timeout(HANG_BOUND)
                            .expect("the driver releases");
                        plain_output(fresh)
                    },
                )
            })
        })
    };
    allocated_rx
        .recv_timeout(HANG_BOUND)
        .expect("the construction interned its fresh payload");
    assert!(
        account.snapshot().active_bytes >= PAYLOAD_LEN,
        "the fresh payload charged ACTIVE while only its construction owns it"
    );
    drop_gate_tx
        .send(())
        .expect("the producer is parked at its drop gate");
    dropped_rx
        .recv_timeout(HANG_BOUND)
        .expect("the older node released inside the build");
    assert!(
        account.snapshot().active_bytes >= PAYLOAD_LEN,
        "the older node's release paid the settled storage pin, not the \
         live construction's ACTIVE charge: {}",
        account.snapshot().active_bytes
    );
    finish_tx
        .send(())
        .expect("the producer is parked in its build");
    let read = producer
        .join()
        .expect("the producer completes its build after the release");
    assert!(matches!(read.value(), QueryResult::Value(_)));
    drop(read);
    drop(harness);
    assert_eq!(
        account.snapshot().total_bytes(),
        0,
        "the isolated owner's teardown drains every class"
    );
}

/// Two producers with constructions open on one shared store: whichever
/// opened LAST must not own the FIRST's fresh allocations. The neighbor B
/// settles first here while A's build is parked with a live fresh payload —
/// B's settle must settle only B's own bytes, leaving A's payload ACTIVE
/// until A itself settles.
#[test]
fn a_settling_neighbor_cannot_drain_a_parked_producers_construction() {
    use verter_type_engine::semantic_query::{LiteralValue, SemanticNodeData};

    let host = Arc::new(host());
    let account = account(usize::MAX / 2);
    let harness = Arc::new(GraphFixture::with_account(StoreAccount::new(Arc::clone(
        &account,
    ))));
    let (entered_a_tx, entered_a_rx) = std::sync::mpsc::channel::<()>();
    let (alloc_gate_tx, alloc_gate_rx) = std::sync::mpsc::channel::<()>();
    let (allocated_tx, allocated_rx) = std::sync::mpsc::channel::<()>();
    let (release_a_tx, release_a_rx) = std::sync::mpsc::channel::<()>();
    let (release_b_tx, release_b_rx) = std::sync::mpsc::channel::<()>();
    let payload = "a".repeat(PAYLOAD_LEN);
    let producer_a = {
        let harness = Arc::clone(&harness);
        let host = Arc::clone(&host);
        std::thread::spawn(move || {
            harness.run(|store| {
                store.execute_cooperative(
                    &*host,
                    named_key("OverlapA"),
                    || panic!("no recursion"),
                    move || {
                        entered_a_tx.send(()).expect("the driver waits");
                        alloc_gate_rx
                            .recv_timeout(HANG_BOUND)
                            .expect("the driver releases the allocation");
                        let fresh = store
                            .intern_node(SemanticNodeData::Literal(LiteralValue::String(payload)));
                        allocated_tx.send(()).expect("the driver waits");
                        release_a_rx
                            .recv_timeout(HANG_BOUND)
                            .expect("the driver releases the build");
                        plain_output(fresh)
                    },
                )
            })
        })
    };
    entered_a_rx
        .recv_timeout(HANG_BOUND)
        .expect("producer A opened its construction");
    let producer_b = {
        let harness = Arc::clone(&harness);
        let host = Arc::clone(&host);
        std::thread::spawn(move || {
            harness.run(|store| {
                store.execute_cooperative(
                    &*host,
                    named_key("OverlapB"),
                    || panic!("no recursion"),
                    || {
                        release_b_rx
                            .recv_timeout(HANG_BOUND)
                            .expect("the driver releases the build");
                        build_with_payload(store, "neighbor")
                    },
                )
            })
        })
    };
    alloc_gate_tx
        .send(())
        .expect("producer A is parked at its allocation gate");
    allocated_rx
        .recv_timeout(HANG_BOUND)
        .expect("producer A interned its fresh payload");
    assert!(
        account.snapshot().active_bytes >= PAYLOAD_LEN,
        "the fresh payload charged ACTIVE in flight"
    );
    release_b_tx
        .send(())
        .expect("producer B is parked in its build");
    let read_b = producer_b
        .join()
        .expect("the neighbor producer completes its build");
    assert!(
        account.snapshot().active_bytes >= PAYLOAD_LEN,
        "the neighbor's settle left the parked construction's fresh payload \
         ACTIVE — it charged the payload to whichever construction opened \
         last: {}",
        account.snapshot().active_bytes
    );
    release_a_tx
        .send(())
        .expect("producer A is parked in its build");
    let read_a = producer_a
        .join()
        .expect("the parked producer completes its build");
    assert!(matches!(
        (read_a.value(), read_b.value()),
        (QueryResult::Value(_), QueryResult::Value(_))
    ));
    drop((read_a, read_b));
    drop(harness);
    assert_eq!(
        account.snapshot().total_bytes(),
        0,
        "the isolated owner's teardown drains every class"
    );
}

/// The other settle order of the same overlap: producer A settles first,
/// while neighbor B stays parked with an EMPTY construction. A's settle
/// must settle A's own fresh payload — not leave it ACTIVE inside B's
/// charge until the unrelated B finishes.
#[test]
fn a_producer_settling_first_settles_its_own_construction_not_its_neighbors() {
    use verter_type_engine::semantic_query::{LiteralValue, SemanticNodeData};

    let host = Arc::new(host());
    let account = account(usize::MAX / 2);
    let harness = Arc::new(GraphFixture::with_account(StoreAccount::new(Arc::clone(
        &account,
    ))));
    let (entered_a_tx, entered_a_rx) = std::sync::mpsc::channel::<()>();
    let (alloc_gate_tx, alloc_gate_rx) = std::sync::mpsc::channel::<()>();
    let (allocated_tx, allocated_rx) = std::sync::mpsc::channel::<()>();
    let (release_a_tx, release_a_rx) = std::sync::mpsc::channel::<()>();
    let (release_b_tx, release_b_rx) = std::sync::mpsc::channel::<()>();
    let payload = "a".repeat(PAYLOAD_LEN);
    let producer_a = {
        let harness = Arc::clone(&harness);
        let host = Arc::clone(&host);
        std::thread::spawn(move || {
            harness.run(|store| {
                store.execute_cooperative(
                    &*host,
                    named_key("OverlapA"),
                    || panic!("no recursion"),
                    move || {
                        entered_a_tx.send(()).expect("the driver waits");
                        alloc_gate_rx
                            .recv_timeout(HANG_BOUND)
                            .expect("the driver releases the allocation");
                        let fresh = store
                            .intern_node(SemanticNodeData::Literal(LiteralValue::String(payload)));
                        allocated_tx.send(()).expect("the driver waits");
                        release_a_rx
                            .recv_timeout(HANG_BOUND)
                            .expect("the driver releases the build");
                        plain_output(fresh)
                    },
                )
            })
        })
    };
    entered_a_rx
        .recv_timeout(HANG_BOUND)
        .expect("producer A opened its construction");
    let producer_b = {
        let harness = Arc::clone(&harness);
        let host = Arc::clone(&host);
        std::thread::spawn(move || {
            harness.run(|store| {
                store.execute_cooperative(
                    &*host,
                    named_key("OverlapB"),
                    || panic!("no recursion"),
                    || {
                        release_b_rx
                            .recv_timeout(HANG_BOUND)
                            .expect("the driver releases the build");
                        build_with_payload(store, "neighbor")
                    },
                )
            })
        })
    };
    alloc_gate_tx
        .send(())
        .expect("producer A is parked at its allocation gate");
    allocated_rx
        .recv_timeout(HANG_BOUND)
        .expect("producer A interned its fresh payload");
    release_a_tx
        .send(())
        .expect("producer A is parked in its build");
    let read_a = producer_a
        .join()
        .expect("the producer completes its build while its neighbor parks");
    assert!(
        account.snapshot().active_bytes < PAYLOAD_LEN,
        "the settling producer's own payload settled with it — not left \
         ACTIVE inside the still-parked neighbor's construction: {}",
        account.snapshot().active_bytes
    );
    release_b_tx
        .send(())
        .expect("producer B is parked in its build");
    let read_b = producer_b
        .join()
        .expect("the parked neighbor completes its build");
    assert!(matches!(
        (read_a.value(), read_b.value()),
        (QueryResult::Value(_), QueryResult::Value(_))
    ));
    drop((read_a, read_b));
    drop(harness);
    assert_eq!(
        account.snapshot().total_bytes(),
        0,
        "the isolated owner's teardown drains every class"
    );
}

/// A fresh construction's OWNED payload cells — the quasis of a template
/// literal, charged by their own cells rather than the arena's granular
/// storage — charge ACTIVE while only the construction owns them, and
/// settle with it. A construction that first-charged them PINNED would
/// never show the payload in an ACTIVE window; flight storage and delivery
/// metadata (a few KiB) cannot cross the payload rail.
#[test]
fn a_fresh_owned_payload_cell_charges_active_under_its_construction() {
    use verter_type_engine::semantic_query::{PayloadArray, SemanticNodeData, SemanticNodeId};
    use verter_type_expr::{OwnedSlice, OwnedStr};

    let host = host();
    let account = account(usize::MAX / 2);
    let harness = GraphFixture::with_account(StoreAccount::new(Arc::clone(&account)));
    let quasi = "q".repeat(PAYLOAD_LEN);
    let template_build =
        |store: &SemanticGraphStore,
         text: &str|
         -> verter_type_engine::project_semantic_dispatch::walk::QueryBuildOutput<
            SemanticNodeId,
        > {
            let node = store.intern_node(SemanticNodeData::TemplateLiteral {
                quasis: OwnedSlice::from(vec![OwnedStr::from(text)]),
                expressions: PayloadArray::from(Vec::new()),
            });
            plain_output(node)
        };
    let owner = account.mint_active_owner();
    let window = account.open_active_window(&owner);
    let read = harness.run(|store| {
        store.execute_cooperative(
            &host,
            named_key("Quasi"),
            || panic!("no recursion"),
            || template_build(store, &quasi),
        )
    });
    let report = window.finish();
    let peak = report
        .attributed()
        .expect("a workload alone on its private account is attributable");
    assert!(
        peak.crossed(PAYLOAD_LEN),
        "the fresh owned quasi charged ACTIVE before publication — flight \
         storage and delivery metadata (a few KiB) cannot cross \
         {PAYLOAD_LEN}: peak {}",
        peak.peak_active_bytes
    );
    assert!(
        report.rise_bytes >= PAYLOAD_LEN,
        "the quasi's own cell rose ACTIVE inside the window"
    );
    assert_eq!(report.active_at_close, 0, "the settle handed it over");
    assert!(matches!(read.value(), QueryResult::Value(_)));

    // Reused-node control: a second construction of the SAME template
    // structurally reuses the live node, so no fresh owned cell charges.
    let second_owner = account.mint_active_owner();
    let second_window = account.open_active_window(&second_owner);
    let second = harness.run(|store| {
        store.execute_cooperative(
            &host,
            named_key("Quasi2"),
            || panic!("no recursion"),
            || template_build(store, &quasi),
        )
    });
    let second_report = second_window.finish();
    let second_peak = second_report
        .attributed()
        .expect("the second workload is attributable to its own owner");
    assert!(
        !second_peak.crossed(PAYLOAD_LEN),
        "a construction that reuses the live template charges no fresh \
         owned payload: peak {}",
        second_peak.peak_active_bytes
    );
    assert!(
        second_peak.crossed(1),
        "the second read's own flight is still real work"
    );
    drop((read, second));
    drop(harness);
    assert_eq!(
        account.snapshot().total_bytes(),
        0,
        "the isolated owner's teardown drains every class"
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
