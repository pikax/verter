# WSP3 evidence index

Selected case IDs: `WSP3-AC1`, `WSP3-AC2`, `WSP3-AC3`, `WSP3-AC-OWNER`, `WSP3-AC-BASIS`, `WSP3-AC-RESOURCE`, `WSP3-AC-EXPOSURE`.

Commands (run on the candidate; this file is not an execution transcript):

- `cargo test -p verter_lsp --lib outbound::tests`
- `cargo test -p verter_lsp --lib documents::diagnostics::tests`
- `cargo test -p verter_lsp --test main outbound_slow_client`

Retired route: diagnostics handed straight to the tower client channel. That channel enqueues a payload on the first poll of its send, so a publication cancelled by close or supersession left its payload queued and it still reached the client. Every publication now goes through `ReplaceableLane`; cancellation withdraws the pending payload.

AC1: a stalled reader and an edit storm across many documents keep the lane within `OutboundBudget`; once the reader resumes, every document ends on its newest complete diagnostics set and never receives an older set after a newer one.

AC2: `$/cancelRequest` and `shutdown` responses are preceded only by what the transport already held; the replaceable backlog stays in the lane.

AC3: a large response reaches a client without partial-result support whole; a diagnostics payload larger than the byte budget is admitted alone and delivered whole.

AC-OWNER: one replaceable writer (`ReplaceableLane`), one producer (`DocumentRegistry::publish_diagnostics`).

AC-BASIS: the epoch/receipt fence is unchanged; the lane removes only superseded payloads.

AC-RESOURCE: no absolute timing, byte or memory figure is claimed; real-client paint and provider process cost are unavailable here.

AC-EXPOSURE: no new protocol operation.
