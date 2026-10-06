# Foreground request settlement observations

Classification and format are defined by
[`docs/arch/semantic-observe.md`](../../../docs/arch/semantic-observe.md).
These rows cover the fields, stores and hooks the foreground request context
and its provider-surface bracket add or change. No counter is added; the one
trace-only item sits behind the crate's default-off `semantic-observe` feature,
which forwards to the session's coordinated opt-in.

## verter_lsp

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `ProviderSurfaceStamp::content_epoch` | `ProviderSurfaceStore::captured_surface_is_current`, the provider-surface bracket every provider-backed foreground answer closes at decode and again at settlement | REQUIRED | Carried by each snapshot; inherited by an identical re-record of a live path, minted fresh from the store's session-monotonic sequence on any content, carrier-source or map change; retires with the last snapshot `Arc` | `src/provider_surface_store/mod.rs` | always |
| `ProviderSurfaceStamp::incarnation` | `ProviderSurfaceStore::captured_surface_is_current` (a close and identical reopen is a different surface) | REQUIRED | Carried by each snapshot; inherited while the path stays live, minted fresh by the first record after the path was absent or closing; retires with the last snapshot `Arc` | `src/provider_surface_store/mod.rs` | always |
| `ProviderSurfaceStamp::owner_epoch` | `ProviderSurfaceStore::captured_surface_is_current` (an owner change that changes back is a different surface) | REQUIRED | Carried by each snapshot; inherited while successive records name the same project owner, minted fresh from the session-monotonic sequence when the owner changes; retires with the last snapshot `Arc` | `src/provider_surface_store/mod.rs` | always |
| `ForegroundRequest::document` (`DocumentSnapshotIdentity`: open incarnation, edit generation, client version, source `Arc`) | `ForegroundRequest::settle` revision gate | REQUIRED | One per in-flight foreground request; pins the admitted source bytes until the request completes or its future is dropped on cancellation | `src/documents/foreground.rs` | always |
| `ForegroundRequest::authority` (`Arc<PublishedRoot>`) | `ForegroundRequest::settle` project-authority gate (identity or same-snapshot republication) | REQUIRED-lifetime | One strong pin per in-flight foreground request; keeps a replaced root alive only until that request completes or is cancelled | `src/documents/foreground.rs` | always |
| `ForegroundRequest::decoded_surfaces` (`Vec<Arc<ProviderSurfaceSnapshot>>`) | `ForegroundRequest::settle` surface gate: every surface a provider answer was decoded through must still be current | REQUIRED-lifetime | Grows by one `Arc` per distinct surface a provider answer of the request is decoded through (deduplicated by pointer); released with the request on completion or cancellation | `src/documents/foreground.rs` | always |
| `ForegroundRequest::dependencies` (`Vec<(canonical id, HostSourceRevisionToken)>`) | `ForegroundRequest::settle` dependency gate: every imported source a native contribution was read from must still be at its recorded host revision | REQUIRED | Grows by one entry per distinct (child, revision) read on the request path; copies only, released with the request | `src/documents/foreground.rs` | always |
| `ACTIVE_REQUEST` task-local (`Arc<ForegroundRequest>`) | `ForegroundRequest::bracket_decoded_surface` / `bracket_dependency`, the hooks the decode bracket and child-contract reads call to join the request's settlement | REQUIRED | Set only while `ForegroundRequest::compute` polls one request's computation; absent for background work, which records nothing | `src/documents/foreground.rs` | always |
| `ForegroundRequest::route` (`ForegroundRoute`), `ResponseClass` and the `foreground answer superseded` debug trace | none — trace payload only; the disposition does not branch on them | OPTIONAL | Copy value per in-flight request; the trace is emitted once per superseded answer | `src/documents/foreground.rs` | `cfg(feature = "semantic-observe")` |
