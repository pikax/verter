# Membership-transition gate inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).
These rows cover the per-source membership-transition gates the external-TS
reconciler serializes behind. No counter, trace or history is added; every item
is current occupancy or ownership state, so nothing sits behind
`semantic-observe`.

## verter_lsp

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `SourceGates::slots` (source → live gate map) | `MembershipReconciler::serialize_source`, entered by `reconcile_source_membership`, `remove_source_membership` and `activate_published_sources` | REQUIRED | One registry per session, owned by `CarrierPublishCoordinator` and shared by every reconciler it builds; an entry exists only while its source has a participant | `src/external_ts/membership_reconciler.rs` | always |
| `SourceGateSlot::gate` (FIFO async mutex) | The same three transitions: same-source transitions run one at a time in arrival order | REQUIRED | Created by the first participant of an idle source; retired with the slot when the last participant leaves | `src/external_ts/membership_reconciler.rs` | always |
| `SourceGateSlot::participants` (holder plus queued waiters) | `SourceGates::leave` retirement decision: the slot is removed exactly when this reaches zero | REQUIRED-lifetime | Incremented by `SourceGates::join`, decremented by `SourceGateParticipant` drop (release, completion or cancellation), both under the registry lock | `src/external_ts/membership_reconciler.rs` | always |
| `SourceGateParticipant` / `SourceGateGuard` (participant and held-gate tokens) | Participation ownership: a waiter's future or a holder's guard owns one participant count; drop is the only release path | REQUIRED-lifetime | One per in-flight transition per source; the held gate is released before the participant leaves | `src/external_ts/membership_reconciler.rs` | always |
| `SOURCE_GATE_SLOT_FLOOR` backing-capacity floor and retirement shrink | Backing allocation bound: after any retirement capacity is at most four times the larger of the live population and the floor | REQUIRED-lifetime | Applied by `SourceGates::leave` when the map is at most a quarter full; amortized, never a global clear | `src/external_ts/membership_reconciler.rs` | always |
| `SourceGates::live_gates`, `participants`, `backing_capacity` (count accessors) | Production count accessors (reached through `CarrierPublishCoordinator::source_gates`); derived from map length, slot count and map capacity under the registry lock | REQUIRED-lifetime | Read-only views of the current registry; no stored counter | `src/external_ts/membership_reconciler.rs`, `src/external_ts/publish_coordinator.rs` | always |
