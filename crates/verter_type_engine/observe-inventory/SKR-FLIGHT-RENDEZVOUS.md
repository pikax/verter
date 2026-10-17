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
| `SemanticGraphStore::flight_rendezvous` (the armed rendezvous scopes, outermost first, one stack per store) | The two hook points below; the innermost is cloned out so no store lock is held while they signal or park | REQUIRED-test | From `arm_flight_rendezvous` until its `FlightRendezvous` guard drops, which removes exactly its own registration in any drop order: a still-armed scope stays armed, and the store disarms once the last guard ends (`semantic_query_memo/{mod,inflight}.rs`) | `test`, `test-support` or `soak-support` |
| `FlightRendezvous`, `FlightRendezvousShared` (key, held flight as a `Weak<FlightCell>`, claim and subscriber counts, parked / released, participant cancellation tokens) | Drivers: `wait_producer_parked`, `wait_subscriber_registered`, `release`, `observe`, `held_flight_is_unlocked`, `held_flight_weak`, `cancel_participants`; `flight_rendezvous_is_disarmed` | REQUIRED-test | Guard-scoped; drop releases a parked producer on completion, unwind or an abandoned schedule. The weak flight reference keeps no flight value or charge alive, lets no later flight reuse its identity, and its failing upgrade is the destruction observation. The participant tokens live only while their participant is parked or registered (`semantic_query_memo/inflight.rs`) | `test`, `test-support` or `soak-support` |
| `ProducerLease::hold_for_flight_rendezvous` at the dispatcher's synchronous cold-build boundary | The first joinable producer of the armed key parks after its claim and before its raw build, holding no memo or flight lock, until released or its own request is cancelled (the same timed cancellation rail a subscriber's wait uses); later producers are counted and run; independent flights are never held. An `Instantiate` key driven outside any drive runs on the continuation runtime and never reaches this boundary, so arming one is refused | REQUIRED-test | One call per produced cold build (`semantic_query_memo/producer.rs`, `project_semantic_dispatch/mod.rs`) | `test`, `test-support` or `soak-support` |
| Subscriber registration note in `Subscription::wait` | Counts a subscriber only after its wait-for edge registered, only for the held flight, signalled with the flight state unlocked (`MutexGuard::unlocked`); the wait re-checks the flight before parking | REQUIRED-test | Per registered subscriber (`semantic_query_memo/producer.rs`) | `test`, `test-support` or `soak-support` |
| `verter_type_engine/soak-support`, forwarded by `verter_session/soak-support` | The soak/tooling runner's explicit opt-in to the rendezvous alone | REQUIRED-test | Default-off; the shipped normal/build closures of the production roots are proven free of it by `observe_feature_closure_is_off_for_production_and_its_dev_unification`; its absence without the opt-in by the `flight_rendezvous_absent_without_support` compile contract; a consumer that names and arms it under only `soak-support` (no `test-support` unification) by the `flight_rendezvous_opt_in_through_soak_support` pass contract | feature |

Public-host proof: `crates/verter_session/tests/cases/g_session/flight_rendezvous.rs`
drives two ordinary `resolve_named_symbol` requests through the rendezvous
(one shared producer, a registered subscriber and positive `joined_waits`),
shows a serial schedule with both callers live fails the same overlap oracle,
and covers unwind, abandonment, nested arming, out-of-order guard drops,
cancelling the parked producer and its subscriber, other keys, other stores
and warm reads — each proving the flight destroyed (the held weak reference
no longer upgrades) and the wait graph back to baseline. Owner-level event
controls for the same seam — a same-key subscriber of another flight,
overlapping guards dropped out of order, and the refusal to arm an
`Instantiate` key — live in
`crates/verter_type_engine/src/semantic_query_memo/tests/flight_rendezvous.rs`.
