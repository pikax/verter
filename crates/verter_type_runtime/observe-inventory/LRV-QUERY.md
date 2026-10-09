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
| `DeliveryLedger` (per-transport persistent map of delivered bytes, each with a stamp and a `DeliveryOrder`) | `DeliveryLedger::dispatch_with` / `bind` (request conversion at the frame's wire position), `ProviderQuery::delivered` / `targeted` (response decode), `DeliveryLedger::settle` | REQUIRED | Owned by one engine incarnation's transport (`TsserverTransport::ledger`, `LspTransport::ledger` shared with its stdin writer); entries recorded when a delivering frame is placed or out-of-band bytes are registered, removed by a close frame or out-of-band withdrawal; dropped with the transport, so a replacement engine starts empty and its replay refills it | `src/provider_query.rs`, `src/tsserver/ipc.rs`, `src/tsgo/ipc.rs` | always |
| `ProviderQuery` (requested bytes + entry stamp, O(1) snapshot of the delivered surface) | every positional tsserver/tsgo query's request conversion and response-range decode | REQUIRED-lifetime | One per in-flight query; shares the snapshot's nodes with the ledger, pins the bytes it reaches only until the query's answer is decoded or its future is dropped | `src/provider_query.rs` | always |
| `QueryAnchor` (tsgo stdin message: requested bytes + capability reply slot) | the tsgo stdin writer's frame placement (`QueryAnchor::place`) | REQUIRED | One per tsgo query frame in a lane; consumed when the writer reaches the frame, or dropped with the lane | `src/tsgo/ipc.rs` | always |
| `TypeProviderError::query_conflict` (one-byte typed marker) | callers distinguishing a coordinate conflict from an engine failure; the error's message names the moved file | REQUIRED | A copy on the returned error; never stored | `src/protocol.rs` | always |
| `AdmissionState::query_proofs` (write set → retained publication + project generation) | `ProviderHub::admit_query`, reusing a generated-unit membership proof for a later read query under the same membership inputs | REQUIRED | One entry per distinct (source, project, unit set) a read query was admitted on; replaced when the membership inputs move; cleared at 4096 entries | `src/provider_hub/admission.rs` | always |

## verter_lsp

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `RequestRoute` (hub, query witness from `bind_query`, optional `admit_query` admission) | `RequestRoute::run`, settling one read query through `check_query` / `check_query_admission` before and after its single engine call | REQUIRED | One per routed read query; released when its answer settles or its future is dropped | `src/tsserver/project_router.rs` | always |
| `PublicationFence` (`Exact` for writes, `Membership` for reads and engine resolution) | `ProjectTsserverProvider::binding_for_source_with_expected`, `engine_for_binding`'s cached engine resolution | REQUIRED | A copy chosen per call; never stored | `src/tsserver/project_router.rs` | always |
