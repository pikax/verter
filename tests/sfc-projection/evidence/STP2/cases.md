# STP2 evidence index

Selected case IDs: `STP2-instance-concrete`, `STP2-instance-generic`, `STP2-instance-explicit`, `STP2-constructor-escape`, `STP2-vue-utilities`, `STP2-not-callable`, `STP2-constructor-inferred`, `STP2-explicit-input-mismatch`.

Commands (run on the candidate; do not treat this file as an execution transcript):

- `node scripts/sfc-projection/verify-node.mjs --node STP2 --engine all --require-all --json`
- `node --test scripts/sfc-projection/verify-node.test.mjs`
- `node roadmap/0.1.0-tama/tools/validate-program-dag.mjs --strict`

Products: `ConstructorCompatibilityEvidence`, `InstanceTypeCompatibilityTable`.

Deletion population this node: empty. Later Vue removal: STP58. Later Svelte removal: STS15. Production constructor emit: STP16. ABI ratification: STP8.

STP2-AC3/AC4 production projection state remains with the untouched owners named in `constructor-compatibility-evidence.json`. Harness-owned probe runs are `incremental=fresh` and close the native API after use.

This file does not claim the cases executed. Copying the charter here is not evidence.
