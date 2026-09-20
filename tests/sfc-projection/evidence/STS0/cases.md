# STS0 evidence index

Input snapshot: this branch's starting revision on `tama_dag/0.1.0-tama/STS0`.

Ratification candidate: the commit that introduced the STS0 products; later review repairs land as later commits on the same branch and are not a new input snapshot.

Engine pins: `tests/sfc-projection/STP1/products/engine-matrix.json` — ts-js `typescript@6.0.3` (`packages/playground/node_modules/typescript`), ts-native `typescript@7.0.2` (`typescript/unstable/sync`). Framework pin: svelte 5.56.10 (`svelte@5.56.10`, root `package.json` devDependencies, resolved at `node_modules/svelte`); semantics authority `packages/framework-conformance-harness/evidence/svelte-options.tsv` (`svelte:CompileOptions`/`runes` supported canonical).

Selected case IDs: `STS0-svelte-inventory`, `STS0-svelte-abi`, `STS0-svelte-pin`, `STS0-policy-lock`.

Commands (run on the candidate; do not treat this file as an execution transcript):

- `node scripts/sfc-projection/verify-node.mjs --node STS0 --engine all --require-all --json`
- `node --test tests/sfc-projection/STS0/svelte-lock.spec.ts`
- `node --test scripts/sfc-projection/verify-node.test.mjs`

Products: `SvelteProjectionPolicy`, `SvelteCurrentFeatureInventory`, `SvelteEngineFrameworkMatrix`.

Charter home divergence: the charter's planned contract home `roadmap/0.1.0-tama/contracts/svelte-typescript-projection.md` was not recreated because `b940abc1e` removed the repository roadmap as database-owned (the divergence STP8 recorded). The ratified Svelte profile contract lives in `tests/sfc-projection/STS0/` (`contract.ts`, `products/`, `protocol.mjs`); the receiving-amendment list the charter expects in that file's section 13 is recorded in `products/svelte-projection-policy.json` (`receivingAmendments`).

Deletion population this node: empty (no standalone legacy route is retired by STS0). Displaced call sites/helpers are recorded as they migrate; final removal stays with STS15 (Svelte) and STP58 (Vue). Full Svelte typing/public declarations/IDE is STS15; module/instance scripts and generic source binders are STS1; runtime compilation stays with SCP and styles with SST; native NCK checking is not a prerequisite or fallback.

STS0-AC2 hover, definition, references and edit participation are observed on the ratified fixture through the shared STP1 harness on both admitted engines; publication and edits stay with the existing provider/LSO snapshot authority (AC3 production state remains with the untouched owners named in `products/svelte-projection-policy.json`). Harness-owned probe runs are `incremental=fresh`, at most one check per file per engine, and close the native API after use. No production hot path, emit, or helper expansion changes (AC4); a contract-only lock needs no invented timing claim.

Negative twins discriminate: a current supported Svelte feature losing its mandatory owning row, a fabricated row outside the canonical feature list, a freshly canonical feature demanded without an owning row, a runes feature mislabeled as legacy, a row citing a missing fixture, empty imported populations, official `recordedRows` drifting from the live TSV, an upstream selected member without an owning row, a harness fixture losing its selected mapping while the fixture remains, the selected members shrinking to one supplied row, a pinned benchmark smoke case member dropped, the benchmarks population citing only its Rust adapter instead of the pinned case inventory, a shipped directive kind (class/style) losing its owning row, a mode-shared feature (stores, transitions/actions/animate, snippets, event attributes, class/style directives) misclassified legacy-only, or export-let marked mode-shared against the mode-paired compiler controls under `probes/modes/` (STS0-svelte-inventory); a Vue constructor, Vue event/model/ref convention, InstanceType-of-a-class requirement, the scanned vue-constructor source twin on the Svelte public shape, a drifted STP7 public shape, STP8 Svelte-owner binding, missing CCA1I backend, or a clean rune probe that does not match pinned `$derived<T>(expression: T): T` (STS0-svelte-abi); a latest-tool or floating-range engine/framework claim, a framework pin diverging from the live root `package.json` pin, an engine outside the STP1 matrix, a shrunken engine denominator, a provenance-less cell, an empty cell set, or a missing admitted engine/framework cell (STS0-svelte-pin); and an unspecified checking/publishing behavior, deletion of an individual supported profile (including `.svelte` module-context), evidence whose file kind/dialect/script context/checkJs marker does not match the row, an option refused without the official fail-closed classification, deletion of an authority-backed refusal, deletion of the unchecked legacy-JS instance profile, or — through the live path — an engine-checked profile whose corrupted projected type is not flagged, a js-unchecked profile reporting type diagnostics, or a renamed published binding whose declaration still publishes (STS0-policy-lock). Every policy profile's evidence fixture is projected and run through the pinned engine's real check and declaration-emit path; the historical legacy- family prefix no longer forces the row semantics.

This file does not claim the cases executed. Copying the charter here is not evidence.
