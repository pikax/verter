# Captured authority views for foreground reads

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).
Rows cover the identity a published workspace root carries, the captured host
authority view foreground requests read and settle through, and the content
evidence that replaced a re-commit-sensitive revision as a validity input. No
counter or trace is added.

## verter_workspace

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `PublishedRoot::authority` (`WorkspaceAuthority`, minted from a process-wide sequence) | `VerterHost::current_authority` and every captured `HostAuthorityView`, which foreground settlement and the LSP child-contract freshness key compare | REQUIRED | Carried by each published root; inherited by a root published as an equivalent republication of the live one under the publication gate, minted fresh by every other construction; never reused within the process | `src/published_state.rs`, `src/engine.rs` | always |

## verter_session

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `HostAuthority` (workspace authority + project generation) | `HostAuthorityView::is_current`, the foreground settlement authority gate and the LSP imported-child-contract freshness key | REQUIRED | A copy; recomputed from the live root and store on every read, retained only inside the captures and keys that hold it | `src/resolver_store.rs` | always |
| `VerterHost::registered_source_whole_hash` (read of the committed source record's whole hash) | the LSP foreground dependency gate, the child-read bracket and the published child-contract, failure and barrel-route snapshots | REQUIRED | No state of its own: reads the scheduler source record, which an identical re-commit after eviction leaves unchanged | `src/host_views.rs` | always |

## verter_lsp

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `SourceFeatureDocumentCapture::expected_source_hash` | `DocumentRegistry::source_feature_document_is_current`, the currency check of an already computed native answer | REQUIRED | One copy per request-local source-feature capture; released with the capture | `src/documents/mod.rs`, `src/documents/analysis.rs` | always |
| `ImportedChildContractFreshnessKey::authority` | `cached_child_public_contract`, `cached_child_public_contract_failure`, `cached_barrel_component_contract` and their publication fences | REQUIRED | Stored with each published child contract, failure and barrel route; replaced with the entry | `src/server/mod.rs`, `src/server/sync_orchestration.rs` | always |
| `ForegroundRequest::dependency_unsettled` | `ForegroundRequest::settle` dependency gate: a child whose content moved across every read of it refuses the answer whatever content it ends at | REQUIRED | One flag per in-flight foreground request; released with the request | `src/documents/foreground.rs`, `src/server/component_resolve.rs` | always |
