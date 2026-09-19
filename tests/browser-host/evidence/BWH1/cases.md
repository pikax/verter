# BWH1 evidence index

Selected case IDs: `BWH1-AC1`, `BWH1-AC2`, `BWH1-AC3`, `BWH1-AC-OWNER`,
`BWH1-AC-BASIS`, `BWH1-AC-RESOURCE`, `BWH1-AC-EXPOSURE`.

Commands (run on the candidate; do not treat this file as an execution
transcript):

- `node --test scripts/browser-host-gate.test.mjs` — fail-closed
  discriminations without WASM or browsers (missing artifact, census
  native import, missing playwright, pass-with-no-tests, empty operation
  array, identity divergence, clean injected evidence).
- `cargo test -p verter_wasm --lib bwh1_native_feasibility_probe -- --exact --nocapture`
  — native same-fixture probe; prints `BWH1_NATIVE_RECEIPT:`.
- `node scripts/browser-host-gate.mjs` — the real browser-host-domain
  gate: wasm32 build, cargo tree census, native probe, Chromium/Firefox/WebKit
  workers.
- `node scripts/browser-host-gate.mjs --skip-build` — same, using an already
  built `packages/wasm/wasm/verter_wasm_bg.wasm`.
- CI runs the skip-build form on the `browser-host` lane after `wasm-build`.

BWH1.1 grounding: the worker loads the published `verter_wasm` artifact and
calls upsert/compileRequest, listSymbols/resolveSymbolWithAudit,
matchCssSelectors, runtime source maps, and resolveTypeWithAudit. Engines
are Chromium, Firefox and WebKit via the existing `@playwright/test` pin.

BWH1.2 grounding: packages/browser-host is a probe/worker facade over
`VerterHost`. analyzeWithAudit remains the throwing wasm stub; no
reduced-semantic TypeInfo is shipped.

BWH1.3 grounding: portability-cut-list.v1.json assigns each seam to a named
existing owner. redesignAmendments is empty on this candidate.

BWH1.4 grounding: scripts/browser-host-gate.mjs is the browser-host-domain
profile. Missing exports, tests, browsers or empty operation arrays fail.

AC-OWNER: final owner `expansion.browser-host`; retirement is BWH3 adopting
the proven seams.

AC-BASIS/AC-RESOURCE/AC-EXPOSURE: no cache/incremental publication change;
metrics are labelled; no new DX0 admission.

Deletion population this node: empty.

This file does not claim the cases executed. Copying the charter here is not
evidence.
