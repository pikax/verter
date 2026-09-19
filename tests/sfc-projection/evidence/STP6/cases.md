# STP6 evidence index

Source revision: `da71a1dfb44208ee8a21af2b4c3c929a288a103d` (captured STP6 implement) on `tama_dag/0.1.0-tama/STP6`.

Engine pins: `tests/sfc-projection/STP1/products/engine-matrix.json` — ts-js `typescript@6.0.3` (`packages/playground/node_modules/typescript`), ts-native `typescript@7.0.2` (`typescript/unstable/sync`). Vue input pin: `vue@3.6.0-rc.5` / `@vue/compiler-sfc@3.6.0-rc.5`.

Selected case IDs: `STP6-package-instance`, `STP6-package-generics`, `STP6-hidden-metadata`, `STP6-closure`, `STP6-decl-map`, `STP6-resolution`.

Commands (run on the candidate; do not treat this file as an execution transcript):

- `node scripts/sfc-projection/verify-node.mjs --node STP6 --engine all --require-all --json`
- `node --test tests/sfc-projection/STP6/packed.spec.ts`
- `node --test scripts/sfc-projection/verify-node.test.mjs`

Products: `PackedConsumerFeasibility`, `PublicDependencyClosurePolicy`.

Deletion population this node: empty. Later Vue removal: STP58. Later Svelte removal: STS15. Production declaration emit: STP16. Full Vue typing: STP59. ABI ratification: STP8.

STP6-AC3/AC4 production projection state remains with the untouched owners named in `packed-consumer-feasibility.json`. Harness-owned pack/install and probe runs are `incremental=fresh` and close native APIs after use.

Negative twins discriminate: importing producer-side unpublished metadata (`@stp6/lib/unpublished-meta`), and following a private virtual path (`@stp6/lib/__virtual_sfc`) or an emitted declaration that captures one. Public packed declarations stay closed and constructor-shaped.

This file does not claim the cases executed. Copying the charter here is not evidence.
