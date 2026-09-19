# STP4 evidence index

Selected case IDs: `STP4-js-unchecked`, `STP4-js-checked`, `STP4-tsx-authored`, `STP4-supplemental-import`, `STP4-external-owner`, `STP4-illegal-vue`.

Commands (run on the candidate; do not treat this file as an execution transcript):

- `node scripts/sfc-projection/verify-node.mjs --node STP4 --engine all --require-all --json`
- `node --test scripts/sfc-projection/verify-node.test.mjs`

Products: `DialectTopologyEvidence`, `ProjectionTopologyDecisionInputs`.

Deletion population this node: empty. Later Vue removal: STP58. Later Svelte removal: STS15. Production JS/JSDoc projection: STP12. ABI ratification: STP8.

STP4-AC3/AC4 production projection state remains with the untouched owners named in `dialect-topology-evidence.json`. Harness-owned probe runs are `incremental=fresh` and close the native API after use.

Negative twins discriminate: importing a compiler-named supplemental as a public module, and treating Vue-illegal script-setup src as accepted because generated TypeScript typechecks.

This file does not claim the cases executed. Copying the charter here is not evidence.
