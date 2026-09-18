# STP5 evidence index

Selected case IDs: `STP5-encoding`, `STP5-guard-duplicate`, `STP5-alias-edit`, `STP5-stale-target`, `STP5-raw-cli`, `STP5-capability`.

Commands (run on the candidate; do not treat this file as an execution transcript):

- `node scripts/sfc-projection/verify-node.mjs --node STP5 --engine all --require-all --json`
- `node --test tests/sfc-projection/STP5/mapper.spec.ts`
- `node --test scripts/sfc-projection/verify-node.test.mjs`
- `node roadmap/0.1.0-tama/tools/validate-program-dag.mjs --strict`

Products: `MapperCapabilityEvidence`, `DiagnosticOriginPolicy`, `ObservationRolePolicy`.

Deletion population this node: empty. Later Vue removal: STP58. Later Svelte removal: STS15.

STP5-AC3/AC4 production projection state remains with the untouched owners named in `tests/sfc-projection/STP5/products/diagnostic-origin-policy.json`. Harness-owned mapper sessions are `incremental=fresh` and close native APIs after use.

Negative twins discriminate: repeated diagnosticDirectives, Alias-as-kebab/Pascal rename, query-file snapshot reuse for a foreign definition, Verter-only postprocessor claimed as stock CLI, 7.1.0-dev version-label capability, and dormant TCM2 complete-capability claims.

This file does not claim the cases executed. Copying the charter here is not evidence.
