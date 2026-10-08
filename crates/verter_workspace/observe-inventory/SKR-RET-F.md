# Content-transition freshness history inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

## verter_workspace

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| Exact transition entries (`FreshnessHistory` exact map: generation + live canonical-lease count) | `last_content_transition_generation`: artifact-only serving gate, store-view artifact-only whole hash, LSP held-revision checks | REQUIRED | Engine-owned; an unleased entry retires into the floor once the retirement queue reaches its trigger; a leased entry lives until its last canonical lease drops | `src/freshness/mod.rs` | always |
| Subtree transition entries (ancestor-indexed prefix map) | Same freshness answer for every canonical under a recorded directory event | REQUIRED | Engine-owned; retires into the floor, folding into leased entries under it first | `src/freshness/mod.rs` | always |
| Monotone floor and observed current generation | Answer for every unleased canonical without retained evidence; caps retirement at the live content generation | REQUIRED | One scalar pair per workspace; only rises | `src/freshness/mod.rs` | always |
| Retirement queue (generation-ordered unleased exact and subtree keys) | Retirement order: a pass pops only the entries it retires | REQUIRED-lifetime | Mirrors the unleased entries; drained by retirement passes | `src/freshness/mod.rs` | always |
| Leased-key index (ordered set of exact keys with at least one canonical lease) | Retiring a subtree folds into only the leased canonicals under it (one range seek); constant-time leased count | REQUIRED-lifetime | Mirrors the leased exact entries; a key leaves with its last lease | `src/freshness/mod.rs` | always |
| View-lease registry (captured generation → count) | Caps the floor at every live request view's captured generation | REQUIRED-lifetime | One record per live `ViewFreshnessLease` | `src/freshness/mod.rs` | always |
| `CanonicalFreshnessLease` / `ViewFreshnessLease` handles | Reader ownership of freshness evidence | REQUIRED-lifetime | RAII; released on drop | `src/freshness/mod.rs` | always |
| `FreshnessResidency` in `WorkspaceResourceSnapshot::freshness_history` (entries, leased, queued, capacities, view leases, floor) | None in production yet: resident-count evidence read by the retirement tests and reported by resource snapshots; the leased count is maintained, not scanned | REQUIRED-lifetime | Derived from current map occupancy under the history read lock; snapshot lives with the caller | `src/freshness/mod.rs`, `src/engine.rs`, `src/traits.rs` | always |
| `FreshnessObserveSnapshot` counters (index probes by lookups, records and retirement folds; retirement passes; retired entries) | none; measurement and tests only | OPTIONAL | History-instance atomics; never reset | `src/freshness/mod.rs` | `cfg(feature = "semantic-observe")` |

## verter_session

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `StoredArtifact::freshness` / `RetiredArtifactVersion` freshness lease | Keeps a stored version's canonical answering from its own evidence for the artifact-only freshness gates | REQUIRED-lifetime | One shared `Arc` per stored version, carried into its retired copy; released when the last copy is reclaimed | `src/file_artifact_store.rs` | always |
| `FileArtifactStore::freshness_readers` | Leases for newly published versions | REQUIRED-lifetime | Replaced on host construction and workspace swap | `src/file_artifact_store.rs` | always |
| `RequestStoreView` view lease | Caps freshness retirement at the request base's captured generation from the moment the view is built | REQUIRED-lifetime | Request-scoped; dropped with the view | `src/resolver_core/request_store_view.rs` | always |
| `VerterHost::lease_content_transition` | Held-revision readers own their canonical's evidence (LSP `PendingProviderReady`, snapshot provider-sync delivery check) | REQUIRED-lifetime | Held exactly as long as the captured revision is compared | `src/host_lifecycle.rs` | always |

## verter_lsp

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `PendingProviderReady` source freshness lease | The background drain's equality recheck of the captured source revision | REQUIRED-lifetime | From the gateway's open-time capture until the pending is confirmed or dropped | `src/external_ts/membership_reconciler.rs` | always |
| Snapshot provider-sync delivery lease | The delivery's `current` recheck of the captured source revision | REQUIRED-lifetime | Function-scoped across the delivery | `src/server/sync_orchestration.rs` | always |
