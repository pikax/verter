# STP3 evidence index

Selected case IDs: `STP3-coupled`, `STP3-wrong-channel`, `STP3-inference-only-channel`, `STP3-order-independent`, `STP3-ordered-merge`, `STP3-fresh-uses`.

Commands (run on the candidate; do not treat this file as an execution transcript):

- `node scripts/sfc-projection/verify-node.mjs --node STP3 --engine all --require-all --json`
- `node --test scripts/sfc-projection/verify-node.test.mjs`

Products: `CoupledInferenceEvidence`, `InferenceWitnessSelection`.

Deletion population this node: empty. Later Vue removal: STP58. Later Svelte removal: STS15. Production use-site inference: STP18. ABI ratification: STP8.

STP3-AC3/AC4 production projection state remains with the untouched owners named in `coupled-inference-evidence.json`. Harness-owned probe runs are `incremental=fresh` and close the native API after use.

Negative twins discriminate: toFixed under U=string, number model write, broad-any constructor, callback-only T lost by split/post-check deferral, sequential first-channel permutation, last-write-wins overwrite of a spread-derived listener, and shared specialization across sibling uses.

This file does not claim the cases executed. Copying the charter here is not evidence.
