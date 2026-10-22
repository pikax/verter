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

### OPTIONAL measurement attribution (unchanged gate)

| item | production consumer | classification | gate |
| --- | --- | --- | --- |
| `active_window`'s thread-local open-window stack (`OPEN_WINDOWS`, `current_window_owner`): `open_active_window` pushes its `(window, owner, account)` and `finish`/drop pops it; `reserve_in_flight` reserves with the innermost window's owner when it is bound to the charged account, and with no provenance otherwise | none (tooling and tests; a measured workload attributes its own ACTIVE bytes) | OPTIONAL | `cfg(any(test, feature = "test-support", feature = "soak-support"))` |
| `Provenance` on the new charges (in-flight reserves, adoptions, handoffs) — zero-sized and never read in a build without the window | none | OPTIONAL | same gate |

Owner proof: `crates/verter_retention/src/{lib,owned,shared,active_window}*_tests.rs`
(unconditional ACTIVE admission past every ceiling, adoption refusability,
handoff transfers, exactly-once release). Engine and public-host owner tests:
`crates/verter_session/src/semantic_query_memo_tests/in_flight_charge_tests.rs`
(publication commits ACTIVE→RETAINED, denied retention hands off ACTIVE→PINNED
complete and uncached, small-ceiling crossing cannot abort the admitted read,
cancelled producer and aborted member release their in-flight charge) and
`crates/verter_session/tests/cases/g_session/in_flight_charge.rs` (a producer
parked through the flight rendezvous holds real ACTIVE bytes while genuinely
in flight; a reopened host's window opens at zero and attributes fresh work).
Public positive proof: `tests/cases/g_session/active_occupancy_window.rs`.
Capture-off availability contract:
`tests/cases/compile-fail/in_flight_charge_available_without_support.rs`.
Frozen soak evidence run: `rev11-l1-soak` (bounded suite green; full-volume
outcomes remain L1's frozen run).
