# Held-answer allocation charge witness inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).
The witness lets a test or the soak/tooling runner read, allocation by
allocation, what a held semantic answer keeps charged on the one
process-local retention account: which allocations it shares, the class and
bytes of each one's own charge, and the bytes each allocation measures. It
reads the existing charge state only: it takes no reservation, adopts no pin,
releases nothing and keeps no handle, so it never changes an admission
decision, a charge, a lifetime or an answer. Production policy reads none of
it, so every row is compiled only under `test`, `test-support` or the
default-off `soak-support` feature; it adds no field, counter or history to
any charged allocation, and the account's current occupancy
(`SemanticRetentionAccount::snapshot`, `VerterHost::retention_snapshot`)
remains REQUIRED production state and is unchanged.

### `verter_retention`

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `ChargeObserver`, `ChargeObservation`, `AllocationCharge`, `AllocationId`, `Reach`, `SharedBlockCharge` | none (tooling and tests; an observation holds addresses and numbers only, so it outlives nothing it names) | OPTIONAL | Per observation; the observer's seen set lives for one build of an observation | `src/witness.rs` | `cfg(any(test, feature = "test-support", feature = "soak-support"))` (ratified narrow exception to `semantic-observe`) |
| `read_node` / `NodeReading` (an owned cell's charge class and bytes, its block share, its registered children, and the first-charge measurement of its value) | none (the observer's only reader of a cell's slot; the slot lock is taken only to copy charge state) | OPTIONAL | Per read | `src/owned.rs` | `cfg(any(test, feature = "test-support", feature = "soak-support"))` (ratified narrow exception to `semantic-observe`) |
| `OwnedHandles::nodes`, `Owned::node`, `CallerHold::identity` | none (identity and cell access for the observer) | OPTIONAL | Borrow-scoped; a cloned cell handle is dropped within the read | `src/owned.rs`, `src/shared.rs` | `cfg(any(test, feature = "test-support", feature = "soak-support"))` (ratified narrow exception to `semantic-observe`) |
| `soak-support` feature, reached through `verter_session_query/soak-support` from `verter_type_engine/soak-support` and `verter_session/soak-support` | none (the soak/tooling runner's opt-in to its seams) | OPTIONAL | Default-off; shipped normal/build closures of the production roots are proven free of it by `observe_feature_closure_is_off_for_production_and_its_dev_unification`, and a planted production edge enabling it is rejected by `support_feature_closure_rejects_a_planted_production_soak_support_edge` | `Cargo.toml` | feature |

### `verter_type_engine`

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `HeldChargeWitness` (`of_flow_return`, `of_read`; payload, cost-receipt and root-metadata sections) | none (tooling and tests) | OPTIONAL | Per observation | `src/semantic_query_memo/charge_witness.rs` | `cfg(any(test, feature = "test-support", feature = "soak-support"))` (ratified narrow exception to `semantic-observe`) |
| `SemanticDeliveryLease::observe_metadata`, `FlowReturnResult::delivery`, `DemandCostReceipt::observe_retention` | none (the witness's readers of a delivery owner's block hold and root-owner cells, a flow return's delivery, and a receipt closure's own charges) | OPTIONAL | Per read | `src/semantic_query_memo/delivery_lease.rs`, `src/semantic_query/flow_return_result.rs`, `src/project_semantic_dispatch/cost_receipt.rs` | `cfg(any(test, feature = "test-support", feature = "soak-support"))` (ratified narrow exception to `semantic-observe`) |
| `soak-support` feature, now forwarding `verter_session_query/soak-support` | none (the soak/tooling runner's opt-in; the witness reads through the retention observer) | OPTIONAL | Default-off; its absence without the opt-in is proven by the `held_charge_witness_absent_without_support` compile contract, and a consumer observing a public flow return under only `verter_session/soak-support` (capture and `semantic-observe` off) by the `held_charge_witness_through_soak_support` pass contract | `Cargo.toml` | feature |

Public-host proof: `crates/verter_session/tests/cases/g_session/held_charge_witness.rs`
observes flow returns delivered by `VerterHost::get_flow_return_type_with_audit`
cold and warm (stable across repeated reads, a fresh host and a large
unrelated live charge), copies sharing every charge, the payload's exact
retained-to-pinned transfer through the public `close` in both release
orders with the bookkeeping charged apart, the remaining reader keeping its
value and charge, and dropped, doubled and partial transfers rejected even
when two executions agree; a dispatch read adds a rail with owned children
and a cost-receipt closure. Owner-level controls live in
`crates/verter_retention/src/witness_tests.rs`.
