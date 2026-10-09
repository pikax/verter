# Query-bound provider coordinate observations

Classification and format are defined by
[`docs/arch/semantic-observe.md`](../../../docs/arch/semantic-observe.md).
These rows cover the state that binds a provider query to the serving engine
incarnation, its project admission and the exact bytes its coordinates are
converted and decoded against. No counter or trace is added, so nothing sits
behind the default-off `semantic-observe` feature.

## verter_type_runtime

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `DeliveryLedger` (per-transport persistent map of delivered bytes, each with a stamp and a `DeliveryOrder`; optional publisher) | `DeliveryLedger::prepare` / `dispatch_with` (request conversion at the frame's wire position against the capability's intended bytes), `BoundQuery::requested` / `target` / `targets` (response decode), `DeliveryLedger::settle` | REQUIRED | Owned by one engine incarnation's transport (`TsserverTransport::ledger`, `LspTransport::ledger` shared with its stdin writer); entries recorded when a delivering frame is placed or out-of-band bytes are registered, removed by a close frame or out-of-band withdrawal; dropped with the transport, so a replacement engine starts empty and its replay refills it | `src/provider_query.rs`, `src/tsserver/ipc.rs`, `src/tsgo/ipc.rs` | always |
| `ProviderQuery` (requester-minted capability: path, intended surface id + bytes, intended foreign targets, hub/router admission) | every `TypeProvider` query method; the adapter's binding refuses bytes other than the intended surface and decodes foreign targets only through the intended targets | REQUIRED-lifetime | One per request attempt; clones share the intent; released when the request ends | `src/provider_query.rs` | always |
| `BoundQuery` (engine incarnation, the admitted `ProviderQuery`, requested bytes + entry stamp + disk observation, O(1) snapshot of the delivered surface, dispatch time, publisher position) | every tsserver/tsgo route's response decode and settlement | REQUIRED-lifetime | One per placed query frame; pins the bytes it reaches only until the answer is decoded or the future is dropped | `src/provider_query.rs` | always |
| `QueryAnchor` (tsgo stdin message: prepared query + frame builder + binding reply slot) | the tsgo stdin writer's frame placement (`QueryAnchor::place`), which converts and binds the query at that position | REQUIRED | One per tsgo query frame in a lane; consumed when the writer reaches the frame, or dropped with the lane | `src/tsgo/ipc.rs` | always |
| `TypeProviderError::query_conflict` (one-byte typed marker) | `provider_query_with_bounded_recovery` (verter_lsp) re-binds a conflicted query to the current surface without a resync; `ProviderHub::run_guarded_with_fallback` never records a conflict as crash evidence; tsserver routes propagate it instead of collapsing it to an empty answer | REQUIRED | A copy on the returned error; never stored | `src/protocol.rs` | always |
| `AdmissionState::query_proofs` (write set → retained publication + project generation) | `ProviderHub::admit_query`, reusing a generated-unit membership proof for a later read query under the same membership inputs | REQUIRED | One entry per distinct (source, project, unit set) a read query was admitted on; replaced when the membership inputs move; cleared at 4096 entries | `src/provider_hub/admission.rs` | always |
| `AdmissionState::query_requests` (queried generated-unit path → read admission) | `generated_query`, reusing a managed read's admission while its membership inputs hold, and `check_query_current` at its settlement | REQUIRED | One entry per queried generated unit; replaced when its membership inputs move; cleared at 4096 entries | `src/provider_hub/admission.rs` | always |

## verter_lsp

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `RequestRoute` (hub, query witness from `bind_query`, optional `admit_query` admission) | `RequestRoute::run`, settling one read query through `check_query` / `check_query_admission` before and after its single engine call | REQUIRED | One per routed read query; released when its answer settles or its future is dropped | `src/tsserver/project_router.rs` | always |
| `PublicationFence` (`Exact` for writes, `Membership` for reads and engine resolution) | `ProjectTsserverProvider::binding_for_source_with_expected`, `engine_for_binding`'s cached engine resolution | REQUIRED | A copy chosen per call; never stored | `src/tsserver/project_router.rs` | always |
| `ReadyFile::published_epoch` (per-row publication epoch in the carrier store) | `CarrierStorePublications::attest`, the publisher authority a tsserver query settles out-of-band bytes on | REQUIRED | Persisted with the row in every journal record and compacted base; kept across identical republication | `src/external_ts/carrier_publish_store.rs` | always |
| `CarrierStorePublications` (incremental follower of the carrier store) | the tsserver adapter's `DeliveryLedger` publisher, installed at spawn by `TsserverEngineInputs` / the test harness | REQUIRED | One per tsserver engine incarnation; folds only records appended since its last read | `src/external_ts/carrier_publish_store.rs` | always |
