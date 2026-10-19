//! The flight rendezvous parks a real cold producer, so it is absent from
//! every build that enables neither `test-support` nor `soak-support`: an
//! ordinary consumer can neither name it nor arm it on a store.

use verter_type_engine::semantic_query::SemanticQueryKey;
use verter_type_engine::semantic_query_memo::{FlightRendezvous, SemanticGraphStore};

fn arm(store: &SemanticGraphStore, key: SemanticQueryKey) {
    let _held: FlightRendezvous<'_> = store.arm_flight_rendezvous(key);
}

fn main() {}
