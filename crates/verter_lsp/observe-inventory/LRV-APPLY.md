# Per-response-class applicability observations

Classification and format are defined by
[`docs/arch/semantic-observe.md`](../../../docs/arch/semantic-observe.md).
These rows cover the fields and hooks that capture the revision of every other
open document a navigation location or an edit is decoded through, and the
negotiated edit shape the delivered edits are bound to. No counter and no trace
is added; every item is current request state or a negotiated capability, so
nothing sits behind `semantic-observe`.

## verter_lsp

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `ForegroundRequest::route` (`ForegroundRoute`) and `ForegroundRoute::class` (`ResponseClass`) | `ForegroundRequest::bracket_target` / `target_source` / `host_target_source`: only a navigation or edit request captures other documents; an informational request is untouched | REQUIRED | Copy value per in-flight request | `src/documents/foreground.rs` | always |
| `ForegroundRequest::edit_support` (`WorkspaceEditSupport`) | `ForegroundRequest::bind_edits` → `action_utils::bind_workspace_edit`, choosing versioned `TextDocumentEdit`s or the route's shape | REQUIRED | Copy value captured at admission from the server's negotiated capability; released with the request | `src/documents/foreground.rs` | always |
| `ForegroundRequest::targets` (`Vec<(Uri, DocumentSnapshotIdentity)>`) | `ForegroundRequest::settle` target gate (every captured target must still be the open revision) and `ForegroundRequest::bind_edits` (the version a delivered edit names) | REQUIRED-lifetime | Grows by one entry per distinct other open document a location or edit of the request is decoded through, captured at its first decode; pins that revision's source `Arc` until the request completes or is cancelled | `src/documents/foreground.rs` | always |
| `ForegroundRequest::target_incoherent` (`AtomicBool`) | `ForegroundRequest::settle` target gate: a decode that found an open target holding other bytes than the answer addressed fails the request | REQUIRED | One flag per in-flight request; set at most once, released with the request | `src/documents/foreground.rs` | always |
| `ForegroundRequest::bracket_target`, `target_source`, `host_target_source`, `mark_target_incoherent` (hooks) | The decode sites of navigation and edit answers: `foreign_ide_context_from_captured`, `classify_captured_api_surface`, the server's `target_source` reader, the host export-span and child-component readers in `server/component_resolve.rs` and `server/nav_features_navigation.rs`, completion resolve and the native component auto-import | REQUIRED | Act on the task-scoped active request only; a no-op for informational routes and outside a foreground request | `src/documents/foreground.rs` | always |
| `VerterLanguageServer::client_applies_versioned_edits` (`AtomicBool`) | `VerterLanguageServer::workspace_edit_support`, read by every foreground admission | REQUIRED | Set once by `initialize` from `workspace.workspaceEdit.documentChanges`; one per server | `src/server/mod.rs` | always |
