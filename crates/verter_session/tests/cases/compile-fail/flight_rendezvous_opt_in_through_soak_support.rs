//! The soak/tooling runner's opt-in: with only `verter_session`'s
//! default-off `soak-support` forwarding enabled — no `test-support`
//! unification — a plain consumer names the rendezvous and arms it on a
//! store. Removing the forwarding, or gating the seam only on
//! `test-support`, fails this contract.

use verter_type_engine::semantic_query::SemanticQueryKey;
use verter_type_engine::semantic_query_memo::{FlightRendezvous, SemanticGraphStore};

fn arm(store: &SemanticGraphStore, key: SemanticQueryKey) {
    let _held: FlightRendezvous<'_> = store.arm_flight_rendezvous(key);
}

fn main() {}
