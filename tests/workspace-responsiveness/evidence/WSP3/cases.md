# WSP3 evidence index

Selected case IDs: `WSP3-AC1`, `WSP3-AC2`, `WSP3-AC3`, `WSP3-AC-OWNER`, `WSP3-AC-BASIS`, `WSP3-AC-RESOURCE`, `WSP3-AC-EXPOSURE`.

Commands (run on the candidate; this file is not an execution transcript):

- `cargo test -p verter_lsp --lib outbound::tests`
- `cargo test -p verter_lsp --lib documents::diagnostics::tests`
- `cargo test -p verter_lsp --test main outbound_slow_client`

Retired route: tower-lsp-server's `Server::serve` writer and its client channel. That channel committed a diagnostics payload on the first poll of its send, so a publication cancelled by close or supersession still reached the client, and control and responses queued in it with no byte account. Verter now owns the writer (`outbound::serve`): every message is accounted in its class — control, response or replaceable — by count and serialized bytes from production until its write completes, and the writer pulls a diagnostics payload only when it is about to write it, re-checking the publication's epoch at that take. The one frame already being written may finish; nothing else that was cancelled is written.

AC1: a stalled reader, an edit storm across many documents, a control backlog and running requests keep each class within its own budget; control producers left waiting are accounted with their bytes; the replaceable lane retains at most its budget plus one waiting payload per document. Once the reader resumes, every document ends on its newest complete diagnostics set and never receives an older set after a newer one. Supersession and close/reopen through the real writer never write a cancelled publication, even when its publisher has not run again.

AC2: `$/cancelRequest` and `shutdown` responses, and the whole control backlog, are preceded only by what the pipe already held and the one frame being written; the diagnostics backlog waits behind them. A server-to-client request is answered while that backlog stands.

AC3: a large response reaches a client without partial-result support whole and is accounted in the response class while the client is stalled; a diagnostics payload larger than the byte budget is admitted alone and delivered whole.

AC-OWNER: one transport writer (`outbound::serve`), one replaceable route (`ReplaceableLane`) with one producer (`DocumentRegistry::publish_diagnostics`); all other server messages go through `Outbound`.

AC-BASIS: the epoch/receipt fence is unchanged; the lane removes only superseded or cancelled payloads, and the storm's final diagnostic sets equal a fresh publication of only the newest edit.

AC-RESOURCE: no absolute timing, byte or memory figure is claimed; real-client paint and provider process cost are unavailable here.

AC-EXPOSURE: no new protocol operation.
