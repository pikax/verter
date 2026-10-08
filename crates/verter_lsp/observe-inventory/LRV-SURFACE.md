# Provider-surface delivery observations

Classification and format are defined by
[`docs/arch/semantic-observe.md`](../../../docs/arch/semantic-observe.md).
These rows cover the fields, stores and hooks that bind a recorded provider
surface to the serving provider's delivery evidence. No counter is added; the
one trace-only item sits behind the crate's default-off `semantic-observe`
feature, which forwards to the session's coordinated opt-in.

Delivery verdicts attest bytes at each local observation. The payload's
acknowledgement retains no engine incarnation or delivery sequence; the store's
incarnation identifies a path lifecycle. Endpoint byte equality cannot identify
an intervening unrecorded A→B→A delivery or a same-byte replay after restart,
and therefore does not bind an answer to the delivery its query evaluated.

## verter_lsp

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `ProviderSurfacePayload::delivery` (`DeliveryCell`, one inline byte) | `ProviderSurfaceStore::delivery_of`, telling a surface that was never delivered (`AwaitingDelivery`) from one whose delivery was lost or overtaken (`DeliveryLost`, `EngineDiverged`) | REQUIRED | Set once, from serving-side evidence only, the first time a verdict observes the serving provider holding the payload's exact bytes; shared by every identical re-record of the payload; retires with the payload's last `Arc` | `src/provider_surface_store/mod.rs` | always |
| `ProviderSurfaceStore` delivery witness (`Arc<dyn ProviderDeliveryWitness>`) | `ProviderSurfaceStore::delivery_of`, read by `captured_surface_is_current` (the foreground decode and settlement bracket) and by the foreground capture | REQUIRED | Bound once at server construction when a provider is configured; one shared handle per store for the life of the server; absent (verdict `Unwitnessed`) when no in-process provider exists | `src/provider_surface_store/mod.rs` | always |
| `ProviderSyncDeliveryWitness` (the provider's per-incarnation application ledger, or the committed membership publication for the membership-only topology) | `ProviderSurfaceStore::delivery_of` | REQUIRED | Owned by the store's witness slot; each read is a local ledger lookup with no provider round trip and no retained state | `src/provider_sync.rs` | always |
| `ProviderSyncState::committed_api_surface` (`CommittedCarrierSurface`) | `ProviderSyncDeliveryWitness`, attesting a public-API companion on the membership-only topology only for the exact bytes the committed publication's receipt fingerprinted | REQUIRED | Installed by the receipt-gated carrier admission from the receipt's API companion fingerprint; kept across a commit that does not re-advertise the API companion only while its path and owner are unchanged; cleared with the owned admission token | `src/provider_sync.rs` | always |
| `ExtensionTypeProvider` application ledger (`applied`) | `TypeProvider::applied_content` for the extension-hosted provider, read by `ProviderSyncDeliveryWitness` | REQUIRED | One entry per file the extension's language service acknowledged; replaced by a newer acknowledged delivery, withdrawn by a refused delivery or a close | `src/extension_provider.rs` | always |
| `SurfaceDelivery` verdict | `ProviderSurfaceStore::captured_surface_is_current` (servable only when `Delivered`) and `VerterLanguageServer::classify_provider_request_surface` | REQUIRED | Copy value computed per verdict; never stored | `src/provider_surface_store/mod.rs` | always |
| `ProviderSurfaceUnavailable` | `VerterLanguageServer::current_file_needs_inline_type_provider_sync`, through `capture_provider_request_surface`: any unavailable surface repairs the requested file before dispatch, or the route answers without the provider | REQUIRED | Copy value per capture; never stored | `src/provider_sync.rs` | always |
| the `provider request surface unavailable` debug trace (`ProviderSurfaceUnavailable` payload) | none — trace payload only; the capture's disposition does not branch on it | OPTIONAL | Emitted once per refused capture | `src/server/sync_orchestration.rs` | `cfg(feature = "semantic-observe")` |
