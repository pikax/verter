//! Owner-level controls for the flight rendezvous seam: the wrong-flight
//! event a same-key subscriber on another flight must not satisfy, the
//! drop order of overlapping guards, and the key families arming accepts.

use std::sync::Arc;
use std::time::Duration;

use crate::semantic_query::SemanticQueryKey;
use crate::semantic_query_memo::inflight::FlightCell;
use crate::semantic_query_memo::{FlightRendezvousMiss, SemanticGraphStore};

const HANG_BOUND: Duration = Duration::from_secs(10);

fn decl_key(canonical: &str, name: &str) -> SemanticQueryKey {
    let owner = verter_type_expr::TopLevelOwnerId::ordinary_file();
    SemanticQueryKey::ResolveDecl(crate::semantic_query::ResolveDeclKey {
        scope: crate::semantic_query::ScopeId {
            canonical_id: verter_type_expr::OwnedStr::from(canonical),
            owner,
            local_scope: None,
            binder_scope_id: crate::semantic_query::BinderScopeId::file_scope(owner),
        },
        name: verter_type_expr::OwnedStr::from(name),
    })
}

/// A subscriber event for the same key on a *different* flight must not
/// satisfy readiness: only the held producer's exact flight counts, so a
/// wrong-flight event leaves readiness unsatisfied and cleanup still
/// completes. The cases public requests cannot stage — two live flights
/// of one key on one store — are driven here at the event level.
#[test]
fn a_same_key_subscriber_of_another_flight_never_satisfies_readiness() {
    let store = SemanticGraphStore::new();
    let key = decl_key("/src/types/icon.ts", "IconBox");
    let flags = crate::resolver_core::resolver_context::RequestFlags::new(
        crate::resolver_core::resolver_context::ProjectGenerationClock::new(Arc::new(
            std::sync::atomic::AtomicU64::new(0),
        )),
    );
    let held = Arc::new(FlightCell::new());
    let other = Arc::new(FlightCell::new());
    // The rendezvous lives inside the scope: a failed assertion unwinds
    // through its drop, which releases the parked producer before the
    // scope joins it, so a planted wrong-flight acceptance fails without
    // hanging cleanup.
    std::thread::scope(|threads| {
        let rendezvous = store.arm_flight_rendezvous(key.clone());
        let shared = store
            .armed_flight_rendezvous()
            .expect("the rendezvous is armed");
        let producer = {
            let shared = Arc::clone(&shared);
            let held = Arc::clone(&held);
            let key = key.clone();
            threads.spawn(move || shared.hold_producer(&key, &held, &flags))
        };
        rendezvous
            .wait_producer_parked(HANG_BOUND)
            .expect("the first producer parks at its cold-build boundary");
        // The same key on a different FlightCell: the wrong flight.
        shared.note_subscriber(&key, &other);
        assert_eq!(
            rendezvous.observe().subscribers,
            0,
            "a same-key subscriber of another flight is never counted"
        );
        assert_eq!(
            rendezvous.wait_subscriber_registered(Duration::ZERO),
            Err(FlightRendezvousMiss::HangBound),
            "the wrong-flight event leaves readiness unsatisfied"
        );
        // The held producer's exact flight satisfies it.
        shared.note_subscriber(&key, &held);
        rendezvous
            .wait_subscriber_registered(HANG_BOUND)
            .expect("the held flight's subscriber registers");
        rendezvous.release();
        producer.join().expect("the held producer returns");
    });
    assert!(store.flight_rendezvous_is_disarmed());
}

/// Overlapping guards may end in any order: dropping the older one leaves
/// the newer registration armed exactly as its own guard left it, the
/// store disarms only after the last guard ends, and a guard that already
/// ended is never re-armed by a later drop.
#[test]
fn overlapping_guards_drop_out_of_order_stay_scoped() {
    let store = SemanticGraphStore::new();
    let older = store.arm_flight_rendezvous(decl_key("/a.ts", "A"));
    let newer = store.arm_flight_rendezvous(decl_key("/b.ts", "B"));
    drop(older);
    assert!(
        !store.flight_rendezvous_is_disarmed(),
        "the still-live newer registration stays armed"
    );
    drop(newer);
    assert!(
        store.flight_rendezvous_is_disarmed(),
        "the store disarms once every guard has ended"
    );
}

/// Arming is for keys the dispatcher's synchronous path produces: an
/// Instantiate read is driven on the continuation runtime, whose frame
/// producers never reach the synchronous cold-build hold, so arming one
/// is refused rather than silently observing nothing.
#[test]
#[should_panic(expected = "never reach the synchronous cold-build hold")]
fn arming_an_instantiate_key_is_refused() {
    let store = SemanticGraphStore::new();
    let owner = verter_type_expr::TopLevelOwnerId::ordinary_file();
    let context = crate::semantic_query::InstantiateContext::non_file(
        crate::semantic_query::ProjectionReductionContext::published(
            crate::semantic_query::ProjectionMode::Expanded,
        ),
        crate::semantic_query::HashValue::default(),
        crate::project_semantic_dispatch::BodySourceWitness::mint_for_unit_tests(),
    );
    let key = SemanticQueryKey::Instantiate(crate::semantic_query::InstantiateKey::new(
        crate::semantic_query::ResolvedDeclSlotIdentity::type_slot_unscoped(
            "/src/types/icon.ts",
            owner,
            "IconBox",
        ),
        Arc::from(Vec::<crate::semantic_query::SemanticNodeId>::new()),
        context,
    ));
    let _ = store.arm_flight_rendezvous(key);
}
