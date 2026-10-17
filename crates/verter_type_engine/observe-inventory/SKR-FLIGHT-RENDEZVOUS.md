# Flight rendezvous inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).
The flight rendezvous lets a test or the soak/tooling runner hold a real cold
producer after it claims its flight, observe a real subscriber registering on
that exact flight, and then release the producer — overlap by construction,
never by caller arrival timing or polled counters. It schedules real work
only: it never changes a query result, a cache identity, admission or
cancellation. Every row is compiled only under `test`, `test-support` or the
default-off `soak-support` feature; no shipped build carries a field, branch
or hook of it.

| Fields, stores and hooks | Consumer | Classification | Lifetime / owner | Gate |
| --- | --- | --- | --- | --- |
| `SemanticGraphStore::flight_rendezvous` (the armed rendezvous, one per store) | The two hook points below; cloned out so no store lock is held while they signal or park | REQUIRED-test | From `arm_flight_rendezvous` until its `FlightRendezvous` guard drops, which restores the previously armed one (`semantic_query_memo/{mod,inflight}.rs`) | `test`, `test-support` or `soak-support` |
| `FlightRendezvous`, `FlightRendezvousShared` (key, held flight as a `Weak<FlightCell>`, claim and subscriber counts, parked / released) | Drivers: `wait_producer_parked`, `wait_subscriber_registered`, `release`, `observe`, `held_flight_is_unlocked`; `flight_rendezvous_is_disarmed` | REQUIRED-test | Guard-scoped; drop releases a parked producer on completion, unwind or an abandoned schedule. The weak flight reference keeps no flight value or charge alive and lets no later flight reuse its identity (`semantic_query_memo/inflight.rs`) | `test`, `test-support` or `soak-support` |
| `ProducerLease::hold_for_flight_rendezvous` at the dispatcher's real cold-build boundary | The first joinable producer of the armed key parks after its claim and before its raw build, holding no memo or flight lock; later producers are counted and run; independent flights are never held | REQUIRED-test | One call per produced cold build (`semantic_query_memo/producer.rs`, `project_semantic_dispatch/mod.rs`) | `test`, `test-support` or `soak-support` |
| Subscriber registration note in `Subscription::wait` | Counts a subscriber only after its wait-for edge registered, only for the held flight, signalled with the flight state unlocked (`MutexGuard::unlocked`); the wait re-checks the flight before parking | REQUIRED-test | Per registered subscriber (`semantic_query_memo/producer.rs`) | `test`, `test-support` or `soak-support` |
| `verter_type_engine/soak-support`, forwarded by `verter_session/soak-support` | The soak/tooling runner's explicit opt-in to the rendezvous alone | REQUIRED-test | Default-off; the shipped normal/build closures of the production roots are proven free of it by `observe_feature_closure_is_off_for_production_and_its_dev_unification`; its absence without the opt-in by the `flight_rendezvous_absent_without_support` compile contract | feature |

Public-host proof: `crates/verter_session/tests/cases/g_session/flight_rendezvous.rs`
drives two ordinary `resolve_named_symbol` requests through the rendezvous
(one shared producer, a registered subscriber and positive `joined_waits`),
shows a serial schedule with both callers live fails the same overlap oracle,
and covers unwind, abandonment, nested arming, other keys, other stores and
warm reads.
