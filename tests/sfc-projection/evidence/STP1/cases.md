# STP1 evidence index

Selected case IDs: `STP1-inventory`, `STP1-zero-selection`, `STP1-clean-twin`, `STP1-types`, `STP1-provenance`, `STP1-harness`.

Commands (run on the candidate; do not treat this file as an execution transcript):

- `node scripts/sfc-projection/verify-node.mjs --node STP1 --engine all --require-all --json`
- `node --test scripts/sfc-projection/verify-node.test.mjs`
- `node roadmap/0.1.0-tama/tools/validate-program-dag.mjs --strict`

Deletion population this node: empty. Later Vue removal: STP58. Later Svelte removal: STS15.

STP1-AC3/AC4 production projection state remains with the untouched owners named in `performance-methodology.json`. Harness-owned probe runs are `incremental=fresh` and close the native API after use.

This file does not claim the cases executed. Copying the charter here is not evidence.
