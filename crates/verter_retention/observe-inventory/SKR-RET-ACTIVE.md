# Real in-flight ACTIVE ownership inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).
This task made in-flight semantic work charge REAL [`ChargeClass::Active`]
bytes on the one process-local account and removed every byte-based refusal
of already-admitted computation. The REQUIRED current ownership/accounting is
capture-off: the ACTIVE rail (`SemanticRetentionAccount::reserve_in_flight`,
the delivery/flight charging, the class transfers) is production state
compiled and charged in every build; the OPTIONAL rows are the measurement
attribution a window reads, compiled only under `test`, `test-support` or the
default-off `soak-support` feature exactly as the accepted witness contract
gated them.

### Production behavior changes (REQUIRED, capture-off, all builds)

| change | production consumer | classification |
| --- | --- | --- |
| `SemanticRetentionAccount::reserve_in_flight(bytes)` — the production ACTIVE entry point: charges unconditionally, never refused, still consumes aggregate headroom; `reserve(ChargeClass::Active, …)` likewise admits unconditionally | the flight cell, producer-protocol deliveries, inline member holds | REQUIRED |
| Removal of the ACTIVE sub-limit (`RetentionLimits::active_ceiling_bytes`, `RetentionRefusal::ActiveExhausted`, the `refusals_active` admission path). `refusals_active` stays a public current count, structurally zero | none (no admission path refuses in-flight computation for bytes) | REQUIRED |
| `ChargeClass::Active` adoption (`RetentionCharge::try_adopt_retained`) decided under the same oversized/pressure rules as a pinned adoption, counting the charge's own bytes as already-occupying; commits the class on success | memo publication (`hold_shared_allocations`, `CallerHold::adopt`) | REQUIRED |
| `FlightCell` storage charged ACTIVE at claim (`inflight.rs`), released when the last holder drops the cell | every cold semantic flight | REQUIRED |
| Producer-protocol deliveries charged in flight (`lease_delivery_in_flight` for a settled producer value and a staged candidate's `lease_candidate`; `DeliveryPhase::InFlight` vs `Settled` in `delivery_lease.rs`), handed off at `SettledProducer::complete` (`hand_off_delivered_allocations`: ACTIVE→PINNED for what no publication adopted) | every cold semantic request | REQUIRED |
| `InlineMemberFlight.hold_value` holds its decided value through `CallerHold::in_flight` until the batched publish or abort ends the hold | SCC batched members | REQUIRED |
| `Owned`/`OwnedHandles` in-flight first charge (`charge_in_flight`) and caller-handoff settle (`hand_off_in_flight`) | delivered payload cells | REQUIRED |
| Fresh CONSTRUCTION ownership (`GranularInFlight` in `granular.rs`, with a running charged figure so filling a construction stays linear in granule count; the arena's open construction scopes and `ConstructionChargeGuard` in `arena.rs`, opened by `open_producer`): the payload blocks, payload heap and slot storage a producer's raw build freshly allocates charge ACTIVE while only the construction owns them — charged to the construction the ALLOCATING THREAD's producer opened (a thread-local construction token keyed by the arena's charge lock), never to whichever construction on the shared arena opened last; every charged allocation records its charging construction (`NodePayload::charge_owner`, the chunk's and each shared-heap entry's owner), so a release pays exactly the freed allocation's owner — the still-open construction, or the settled storage pin once its settlement folded the remainder in — and the producer's settle hands the remainder ACTIVE→PINNED into the arena's storage pin, one granule at a time, exactly once, on every exit. Owned payload cells a construction freshly reaches (template quasis, owned names) charge ACTIVE by their own cells through the construction and settle with it | every cold semantic construction | REQUIRED |
| `commit_retained` no longer moves in-flight bytes into RETAINED: an ACTIVE input is a contract violation in every build, and in-flight charges reach retention only through the refusable `try_adopt_retained` / `CallerHold::adopt` | cache-hold minting (`CacheHold::from_retained`) | REQUIRED |

### OPTIONAL measurement attribution (unchanged gate)

| item | production consumer | classification | gate |
| --- | --- | --- | --- |
| `active_window`'s open-window registry (`OPEN_WINDOWS`, `current_window_owner`): each measuring thread's stack of `(window, owner, account)`; `open_active_window` pushes onto the opening thread's stack and `finish`/drop removes EXACTLY that `(account, window)` entry — from the opening thread's stack, whichever thread closes it, in any close order; `reserve_in_flight` reserves with the innermost window's owner when it is bound to the charged account, and with no provenance otherwise | none (tooling and tests; a measured workload attributes its own ACTIVE bytes) | OPTIONAL | `cfg(any(test, feature = "test-support", feature = "soak-support"))` |
| `Provenance` on the new charges (in-flight reserves, adoptions, handoffs) — zero-sized and never read in a build without the window | none | OPTIONAL | same gate |

Owner proof: `crates/verter_retention/src/{lib,owned,shared,active_window}*_tests.rs`
(unconditional ACTIVE admission past every ceiling, adoption refusability,
handoff transfers, exactly-once release, exact window-registration removal in
any close order, account or thread). Engine and public-host owner tests:
`crates/verter_session/src/semantic_query_memo_tests/in_flight_charge_tests.rs`
(publication commits ACTIVE→RETAINED, denied retention hands off ACTIVE→PINNED
complete and uncached, small-ceiling crossing cannot abort the admitted read,
cancelled producer and aborted member release their in-flight charge, a cold
construction's fresh payload charges ACTIVE with a reused-node control, an
older node's release inside a live construction keeps its ACTIVE charge,
overlapping producers on one store own their constructions separately through
both settle orders, a fresh owned payload cell charges ACTIVE with a
reuse control,
an inline member's held value charges beyond its flight storage) and
`crates/verter_session/tests/cases/g_session/in_flight_charge.rs` (a producer
parked through the flight rendezvous holds real ACTIVE bytes while genuinely
in flight; a reopened host's window opens at zero and attributes fresh work —
one full granule of freshly constructed graph storage).
Public positive proof: `tests/cases/g_session/active_occupancy_window.rs`.
Capture-off availability contract:
`tests/cases/compile-fail/in_flight_charge_available_without_support.rs`.
Frozen soak evidence run: `rev11-l1-soak` (bounded suite green; full-volume
outcomes remain L1's frozen run).
