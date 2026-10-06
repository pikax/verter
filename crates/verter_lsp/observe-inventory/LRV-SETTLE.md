# Foreground request settlement observations

Classification and format are defined by
[`docs/arch/semantic-observe.md`](../../../docs/arch/semantic-observe.md).
These rows cover the fields, stores and hooks the foreground request context
and its provider-surface bracket add or change. No optional counter is added.

## verter_lsp

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `ProviderSurfaceStamp::content_epoch` | `ProviderSurfaceStore::captured_surface_is_current`, the provider-surface bracket every provider-backed foreground answer closes before it is mapped | REQUIRED | Carried by each snapshot; inherited by an identical re-record of a live path, minted fresh from the store's session-monotonic sequence on any content, carrier-source or map change; retires with the last snapshot `Arc` | `src/provider_surface_store/mod.rs` | always |
| `ProviderSurfaceStamp::incarnation` | `ProviderSurfaceStore::captured_surface_is_current` (a close and identical reopen is a different surface) | REQUIRED | Carried by each snapshot; inherited while the path stays live, minted fresh by the first record after the path was absent or closing; retires with the last snapshot `Arc` | `src/provider_surface_store/mod.rs` | always |
| `ForegroundRequest::document` (`DocumentSnapshotIdentity`: open incarnation, edit generation, client version, source `Arc`) | `ForegroundRequest::settle` revision gate | REQUIRED | One per in-flight foreground request; pins the admitted source bytes until the request completes or its future is dropped on cancellation | `src/documents/foreground.rs` | always |
| `ForegroundRequest::authority` (`Arc<PublishedRoot>`) | `ForegroundRequest::settle` project-authority gate (identity or equivalent republication) | REQUIRED-lifetime | One strong pin per in-flight foreground request; keeps a replaced root alive only until that request completes or is cancelled | `src/documents/foreground.rs` | always |
| `ForegroundRequest::route` (`ForegroundRoute`) and `ResponseClass` | Disposition contract per route; `foreground answer superseded` debug trace | REQUIRED | Copy value per in-flight request | `src/documents/foreground.rs` | always |
