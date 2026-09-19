# WSP1 evidence index

Selected case IDs: `WSP1-AC1`, `WSP1-AC2`, `WSP1-AC3`, `WSP1-AC-OWNER`, `WSP1-AC-BASIS`, `WSP1-AC-RESOURCE`, `WSP1-AC-EXPOSURE`.

Commands (run on the candidate; do not treat this file as an execution transcript):

- `node --test tests/workspace-responsiveness/WSP1/wsp1.test.mjs`
- `cargo test -p verter_lsp --lib interaction_trace -- --test-threads=1`
- `pnpm --filter @verter/lsp-test-client test`
- `pnpm --filter @verter/dx-harness exec vitest --run test/protocolReplay.test.ts`

Deletion population this node: empty. Additive instrumentation; no route retired.

AC1: a stalled `LspClient` reader keeps unread bytes after the fake server writes `server-complete`; first blocked server stage is `outbound_enqueued`. Client decode/apply/paint are not inferred.

AC2: `discriminateThroughputVsQuery` labels payload-only serialize/outbound growth as throughput and provider-work dwell as query.

AC3: `rejectReplay` and the live harness fail-closed on dropped diagnostics or a missing provider.

AC-OWNER: final owner `expansion.workspace-responsiveness`; WSP1L receives client stages; Issue 93 stays unreproduced.

AC-BASIS: no cache/mapping/snapshot/result-handle change; incremental-vs-fresh waits on a recorded reference machine (population still empty).

AC-RESOURCE: RSS and client paint labelled unavailable; no guessed zeros; no real-client claim.

AC-EXPOSURE: two inspection operations registered with `promotionBlocked: true` until DX1 executable exposure.

This file does not claim the cases executed. Copying the charter here is not evidence.
