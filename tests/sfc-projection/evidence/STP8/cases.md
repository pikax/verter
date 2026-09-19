# STP8 evidence index

Input snapshot (job baseline, STP7 merge base of this branch): `6e777f11e3db1d8615d813d36d161aaa986c6dc8` on `tama_dag/0.1.0-tama/STP8`.

Ratification candidate: `95175044e97296c6d07045e5e4d4fdc562904e50` (the commit that introduced the STP8 products; review repairs land as later commits on the same branch and are not a new input snapshot).

Engine pins: `tests/sfc-projection/STP1/products/engine-matrix.json` — ts-js `typescript@6.0.3` (`packages/playground/node_modules/typescript`), ts-native `typescript@7.0.2` (`typescript/unstable/sync`). Vue pin: `vue@3.6.0-rc.5` (root `package.json` devDependencies, matrix-admitted lane `vue-3.6`).

Selected case IDs: `STP8-complete-evidence`, `STP8-partial-ratify`, `STP8-abi-contamination`, `STP8-inference-contract`.

Commands (run on the candidate; do not treat this file as an execution transcript):

- `node scripts/sfc-projection/verify-node.mjs --node STP8 --engine all --require-all --json`
- `node --test tests/sfc-projection/STP8/abi-ratify.spec.ts`
- `node --test scripts/sfc-projection/verify-node.test.mjs`

Products: `AcceptedProjectionArchitecture`, `AcceptedVueConstructorABI`, `AcceptedTopologyMatrix`, `HelperABI` v1.

Charter home divergence: the charter's planned contract home `roadmap/0.1.0-tama/contracts/sfc-typescript-projection.md` was not recreated because `b940abc1e` removed the repository roadmap as database-owned. The ratified source contract lives in `tests/sfc-projection/STP8/` (`contract.ts`, `products/`, `protocol.mjs`); the receiving-amendment list the charter expects in that file's section 13 is recorded in `products/accepted-projection-architecture.json` (`receivingAmendments`).

Deletion population this node: empty (no standalone legacy route is retired by STP8). Displaced call sites/helpers are recorded as they migrate; final removal stays with STP58 (Vue) and STS15 (Svelte). Production constructor emit: STP16. Specialization runtime: STP18. Full Vue typing/public declarations/IDE: STP59.

STP8-AC3/AC4 production projection state remains with the untouched owners named in `tests/sfc-projection/STP8/products/accepted-vue-constructor-abi.json`. Harness-owned probe runs are `incremental=fresh`, at most one check per file per engine, and close the native API after use.

Negative twins discriminate: a TypeScript 5.8-only toy engine pin or shrunken engine denominator standing in for the selected real engines/Vue types, a checker-only public surface that respells `InstanceType<typeof Comp>` or remaps Vue utility behavior, a `utilitiesUntouched` claim with no createApp/h/TSX recipe (or whose wrong-prop dirty twin stays green under a respelled props surface) to observe it, an event/slot inference contributor postponed until after specialization, a runtime second-checker witness, an inference transaction that restates a parallel channel catalog instead of joining the STP3 `InferenceWitnessSelection` selection, and a fabricated or uncited feasibility row (a case its cited ledger does not structurally record). A predecessor gaining a mandatory case, a charter §4 named predecessor product going missing or losing its schema, a predecessor ledger emptied of its cases, or a frozen topology field diverging from the predecessor policy product it freezes likewise fails `STP8-complete-evidence`.

AC2 edit application remains with the existing provider/LSO snapshot authority. This lock records hover, definition, references and edit participation on the ratified fixture through the shared harness; it does not add a production edit applicator.

This file does not claim the cases executed. Copying the charter here is not evidence.
