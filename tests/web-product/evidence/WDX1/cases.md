# WDX1 evidence index

Selected case IDs: `WDX1-AC1`, `WDX1-AC2`, `WDX1-AC3`, `WDX1-AC4`, `WDX1-AC5`, `WDX1-delivery`.

Commands (run on the candidate; do not treat this file as an execution transcript):

- `node tests/web-product/WDX1/verify.mjs`
- `node --test tests/web-product/WDX1/wdx1.test.mjs`
- `node --test tests/web-product/WDX0/wdx0.test.mjs` (predecessor harness stays green)
- `node roadmap/0.1.0-tama/tools/validate-program-dag.mjs --strict`
- `node --test roadmap/0.1.0-tama/tools/implementation-ledger.test.mjs`
- the remaining `targeted-domain` final profile command (see `catalogs/gate-profiles.toml`)

Deletion population this node: empty. The boundary is additive fixture and routing data over the existing `tests/web-product` conformance harness; no route is displaced or retired by WDX1.

AC1 grounding: the five authored fixture cases (`tests/web-product/WDX1/fixtures/`, one per charter domain: mixed-framework, css, worker, configuration, security) each carry source/profile/build identities, per-domain expected results over the closed §5 capability-dimension vocabulary, and producers drawn from the WDX0 disposition-matrix final owners. The 41-row `scenario-coverage-matrix.v1.json` routes every recommendation: rows with fixture cases pin them, rows without record `pendingScenarioOwner` on their own head — silence is not a disposition.

AC2 grounding: the worker fixture pins one terminal-receipt-binding recording whose `basis.sourceDigest` is content-addressed to the authored sources (CRLF-normalized sha256 over sorted `path:hash` lines, recomputed on every validation). The `stale-recording` twins prove the discriminator in memory (tampered digest) and on disk (an authored snapshot edit under a copied fixture tree). `premature-runtime-claim` twins keep recordings static-proof, non-qualifying, `pending` and engine/host-unbound while their producer terminal (ENV9) is absent from the live implementation ledger, and the landed-terminal twin proves a stale recording still validates no runtime claim.

AC3 grounding: the fixture index is static and recomputed fresh each run. Deterministic ordering is proven by `canonicalRouting` equality under perturbed discovery order; partial fixtures are rejected (`partial-fixture`); the fresh/incremental, edit/revert and cancellation cases are recorded as not applicable with rationale in `lifecycleRouting`, whose downstream owners are WDX2 and WDX3 on real artifacts.

AC4 grounding: this index plus the fixture identities/profile pins are the node's public delivery. No VIM/DX capability row, catalog family or support claim is added: every routing row keeps `claimBasis: none`, the live catalog must remain exactly the historical vue/svelte families (drift is a failing case), and uncertainty/migration notes live in the coverage product.

AC5 grounding: no budget is bound — the harness is synchronous static validation over small authored files; the ratified performance methodology binds when a producing train first measures (`resourceRationale` in the coverage product). No obsolete path is retired (additive boundary, empty deletion population).

This file does not claim the cases executed. Copying the charter here is not evidence.
