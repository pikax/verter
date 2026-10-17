//! A flight rendezvous holds a real cold producer at its build boundary
//! until a real subscriber has registered on that producer's flight, so two
//! ordinary public named-symbol requests overlap by construction rather than
//! by arrival timing. Every wait is an event on the rendezvous; the hang bound
//! only stops a broken schedule from hanging the test.

use std::sync::Arc;
use std::time::Duration;

use verter_session::{HostConfig, UpsertRequest, VerterHost};
use verter_type_engine::semantic_query::{
    BinderScopeId, ResolveDeclKey, ScopeId, SemanticNodeData, SemanticNodeId, SemanticQueryKey,
};
use verter_type_engine::semantic_query_memo::{
    FlightRendezvousMiss, FlightRendezvousObservation, SemanticGraphStore,
};

const ICON: &str = "/src/types/icon.ts";
const HANG_BOUND: Duration = Duration::from_secs(10);

fn host() -> VerterHost {
    let host = VerterHost::new_standalone(HostConfig::default());
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(ICON.to_string()),
        input_id: ICON.to_string(),
        source: Arc::from(
            "export interface IconProps { name: string; size: number }\n\
             export interface IconBox { props: IconProps; label: string }\n",
        ),
        file_language: verter_session::LanguageRegistry::global()
            .classify_static(ICON)
            .static_resolution(),
        aliases: Vec::new(),
    });
    host
}

/// The memo key a public named-symbol request for `name` claims first.
fn resolve_decl(name: &str) -> SemanticQueryKey {
    let owner = verter_type_expr::TopLevelOwnerId::ordinary_file();
    SemanticQueryKey::ResolveDecl(ResolveDeclKey {
        scope: ScopeId {
            canonical_id: verter_type_expr::OwnedStr::from(ICON),
            owner,
            local_scope: None,
            binder_scope_id: BinderScopeId::file_scope(owner),
        },
        name: verter_type_expr::OwnedStr::from(name),
    })
}

/// The ordinary public request, validated: it resolves `IconBox` to its
/// object body.
fn request_icon_box(host: &VerterHost) -> SemanticNodeId {
    let graph = host.project_type_store().semantic_graph();
    let answer = host
        .resolve_named_symbol(ICON, "IconBox", None)
        .expect("IconBox resolves");
    let data = graph.node_data(answer.id()).expect("the answer is live");
    assert!(
        matches!(data.as_ref(), SemanticNodeData::Object(_)),
        "IconBox resolves to its object body, got {data:?}"
    );
    answer.id()
}

fn joined_waits(graph: &SemanticGraphStore) -> u64 {
    graph.stats_snapshot().joined_waits
}

/// The overlap the rendezvous exists to prove: one shared cold producer of
/// the key, a subscriber registered on its flight, and a wait that actually
/// joined it.
fn overlap_oracle(observed: FlightRendezvousObservation, joined_waits: u64) -> Result<(), String> {
    if observed.producer_claims != 1 {
        return Err(format!(
            "expected one shared producer, saw {} claims",
            observed.producer_claims
        ));
    }
    if observed.subscribers == 0 {
        return Err("no subscriber registered on the producer's flight".into());
    }
    if joined_waits == 0 {
        return Err("no wait joined the producer".into());
    }
    Ok(())
}

#[test]
fn a_held_cold_producer_is_joined_by_a_real_subscriber() {
    let host = host();
    let graph = host.project_type_store().semantic_graph();
    let key = resolve_decl("IconBox");
    let joins_before = joined_waits(graph);

    let (observed, answers) = std::thread::scope(|threads| {
        let rendezvous = graph.arm_flight_rendezvous(key.clone());
        let producer = threads.spawn(|| request_icon_box(&host));
        rendezvous
            .wait_producer_parked(HANG_BOUND)
            .expect("the first request parks at its cold build");
        // The parked producer holds neither its flight nor the flight table.
        assert_eq!(rendezvous.held_flight_is_unlocked(), Some(true));
        assert!(graph.test_inflight_table_is_unlocked());
        let subscriber = threads.spawn(|| request_icon_box(&host));
        rendezvous
            .wait_subscriber_registered(HANG_BOUND)
            .expect("the second request subscribes to the held flight");
        rendezvous.release();
        let answers = [
            producer.join().expect("the producer answers"),
            subscriber.join().expect("the subscriber answers"),
        ];
        (rendezvous.observe(), answers)
    });

    assert!(graph.flight_rendezvous_is_disarmed());
    assert_eq!(
        overlap_oracle(observed, joined_waits(graph) - joins_before),
        Ok(())
    );
    assert_eq!(answers[0], answers[1], "both requests share one answer");
    assert_eq!(request_icon_box(&host), answers[0], "a warm read agrees");
    assert_eq!(
        graph.test_inflight_strong_count(&key),
        0,
        "no flight outlives the requests"
    );
}

#[test]
fn a_serial_schedule_with_concurrent_callers_fails_the_overlap_oracle() {
    let host = host();
    let graph = host.project_type_store().semantic_graph();
    let joins_before = joined_waits(graph);

    let (observed, answers) = std::thread::scope(|threads| {
        let rendezvous = graph.arm_flight_rendezvous(resolve_decl("IconBox"));
        let (go, start) = std::sync::mpsc::channel::<()>();
        let host = &host;
        let producer = threads.spawn(|| request_icon_box(host));
        let subscriber = threads.spawn(move || {
            start.recv().expect("the driver starts the second caller");
            request_icon_box(host)
        });
        rendezvous
            .wait_producer_parked(HANG_BOUND)
            .expect("the first request parks at its cold build");
        // Serial: the producer finishes before the second caller starts.
        rendezvous.release();
        let first = producer.join().expect("the producer answers");
        go.send(()).expect("the second caller is waiting");
        let second = subscriber.join().expect("the second caller answers");
        (rendezvous.observe(), [first, second])
    });

    assert_eq!(answers[0], answers[1]);
    assert_eq!(observed.producer_claims, 1);
    assert_eq!(observed.subscribers, 0, "a warm read never registers");
    let joined = joined_waits(graph) - joins_before;
    assert_eq!(joined, 0);
    assert!(
        overlap_oracle(observed, joined).is_err(),
        "a serial schedule must not pass for overlap"
    );
}

#[test]
fn a_failed_assertion_while_the_producer_is_held_releases_it_and_retains_nothing() {
    let host = host();
    let graph = host.project_type_store().semantic_graph();
    let key = resolve_decl("IconBox");
    let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        std::thread::scope(|threads| {
            let rendezvous = graph.arm_flight_rendezvous(key.clone());
            threads.spawn(|| request_icon_box(&host));
            rendezvous
                .wait_producer_parked(HANG_BOUND)
                .expect("the request parks at its cold build");
            // The scope joins the parked producer only after the unwind has
            // dropped the rendezvous and so released it.
            panic!("a planted assertion failure while the producer is held");
        })
    }));
    assert!(unwound.is_err());
    assert!(graph.flight_rendezvous_is_disarmed());
    assert_eq!(graph.test_inflight_strong_count(&key), 0);
    request_icon_box(&host);
}

#[test]
fn an_abandoned_rendezvous_releases_its_parked_producer() {
    let host = host();
    let graph = host.project_type_store().semantic_graph();
    let key = resolve_decl("IconBox");
    let answer = std::thread::scope(|threads| {
        let rendezvous = graph.arm_flight_rendezvous(key.clone());
        let producer = threads.spawn(|| request_icon_box(&host));
        rendezvous
            .wait_producer_parked(HANG_BOUND)
            .expect("the request parks at its cold build");
        // The driver cancels its schedule without releasing.
        drop(rendezvous);
        producer.join().expect("the released producer answers")
    });
    assert!(graph.flight_rendezvous_is_disarmed());
    assert_eq!(graph.test_inflight_strong_count(&key), 0);
    assert_eq!(request_icon_box(&host), answer);
}

#[test]
fn a_nested_rendezvous_restores_the_one_it_replaced() {
    let host = host();
    let graph = host.project_type_store().semantic_graph();
    let outer = graph.arm_flight_rendezvous(resolve_decl("IconBox"));
    outer.release();
    drop(graph.arm_flight_rendezvous(resolve_decl("IconProps")));
    assert!(!graph.flight_rendezvous_is_disarmed());
    request_icon_box(&host);
    assert_eq!(
        outer.observe().producer_claims,
        1,
        "the outer one is armed again"
    );
    drop(outer);
    assert!(graph.flight_rendezvous_is_disarmed());
}

#[test]
fn other_keys_other_stores_and_warm_reads_never_satisfy_a_rendezvous() {
    let host = host();
    let other = self::host();
    let graph = host.project_type_store().semantic_graph();
    let other_graph = other.project_type_store().semantic_graph();
    let rendezvous = graph.arm_flight_rendezvous(resolve_decl("IconBox"));
    // Released up front, so a producer it wrongly matched could not park.
    rendezvous.release();

    // A different key on this store.
    host.resolve_named_symbol(ICON, "IconProps", None)
        .expect("IconProps resolves");
    // The same key overlapping on another store, under that store's own
    // rendezvous.
    std::thread::scope(|threads| {
        let theirs = other_graph.arm_flight_rendezvous(resolve_decl("IconBox"));
        let producer = threads.spawn(|| request_icon_box(&other));
        theirs
            .wait_producer_parked(HANG_BOUND)
            .expect("the other store's request parks");
        let subscriber = threads.spawn(|| request_icon_box(&other));
        theirs
            .wait_subscriber_registered(HANG_BOUND)
            .expect("the other store's subscriber registers");
        theirs.release();
        producer.join().expect("the producer answers");
        subscriber.join().expect("the subscriber answers");
    });
    assert_eq!(
        rendezvous.observe(),
        FlightRendezvousObservation {
            producer_claims: 0,
            subscribers: 0,
            parked: false,
            released: true,
        }
    );

    // The key itself cold, then warm: only the cold build is a claim, and
    // neither read is a subscriber.
    request_icon_box(&host);
    request_icon_box(&host);
    let observed = rendezvous.observe();
    assert_eq!((observed.producer_claims, observed.subscribers), (1, 0));
    assert_eq!(
        rendezvous.wait_subscriber_registered(Duration::ZERO),
        Err(FlightRendezvousMiss::HangBound)
    );
}
