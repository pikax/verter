# WSP1B evidence index

Selected case IDs: `WSP1B-AC1`, `WSP1B-AC2`, `WSP1B-AC3`, `WSP1B-AC-OWNER`, `WSP1B-AC-BASIS`, `WSP1B-AC-RESOURCE`, `WSP1B-AC-EXPOSURE`.

Commands (run on the candidate; do not treat this file as an execution transcript):

- `node --test tests/workspace-responsiveness/WSP1B/wsp1b.test.mjs`
- `pnpm --filter @verter/dx-harness exec vitest --run test/earlyVerification.test.ts`

Reference-machine capture (not hermetic CI):

- `node tests/workspace-responsiveness/WSP1B/capture-early-verification.mjs --lapce <instrumented lapce> --volt <volt dir> --server <verter-lsp>`

Deletion population this node: empty. Consumes WSP1/WSP1L instrumentation; no route retired.

AC1: a server-immediate + UI-stall large run fails even when the WSP1L two-file smoke is green.

AC2: required typed answers and diagnostic counts must match the captured semantic oracle with required features enabled.

AC3: a two-file smoke or protocol smoke cannot qualify the large-project scenario (2615 Vue files, WSP equal-work synthetic slice equivalent to the unlabeled PrimeVue corpus).

AC-OWNER: final owner `expansion.workspace-responsiveness`; WSP1A issue-93 repair obligation is retired as unreproduced / not planned, not as a fix.

AC-BASIS: no cache/mapping/snapshot/result-handle change; stale runs refused.

AC-RESOURCE: unavailable provider CPU / retained memory labelled unknown; no guessed zeros; real-client paint required for a qualified claim.

AC-EXPOSURE: two inspection operations registered with `promotionBlocked: true` until DX1 executable exposure.

This file does not claim the cases executed. Copying the charter here is not evidence.
