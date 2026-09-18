# STP5 evidence index

Source revision: `aa97390228ce35caa42e855e1ad4e9ebfc9c9cb7` (captured STP5 implement) on `tama_dag/0.1.0-tama/STP5`.

Engine pins: `tests/sfc-projection/STP1/products/engine-matrix.json` — ts-js `typescript@6.0.3` (`packages/playground/node_modules/typescript`), ts-native `typescript@7.0.2` (`typescript/unstable/sync`). Vue input pin: `vue@3.6.0-rc.5` / `@vue/compiler-sfc@3.6.0-rc.5`.

Selected case IDs: `STP5-encoding`, `STP5-guard-duplicate`, `STP5-alias-edit`, `STP5-stale-target`, `STP5-raw-cli`, `STP5-capability`.

Commands (run on the candidate; do not treat this file as an execution transcript):

- `node scripts/sfc-projection/verify-node.mjs --node STP5 --engine all --require-all --json`
- `node --test tests/sfc-projection/STP5/mapper.spec.ts`
- `node --test scripts/sfc-projection/verify-node.test.mjs`
- `node roadmap/0.1.0-tama/tools/validate-program-dag.mjs --strict`

Products: `MapperCapabilityEvidence`, `DiagnosticOriginPolicy`, `ObservationRolePolicy`.

Deletion population this node: empty. Later Vue removal: STP58. Later Svelte removal: STS15.

STP5-AC3/AC4 production projection state remains with the untouched owners named in `tests/sfc-projection/STP5/products/diagnostic-origin-policy.json`. Harness-owned mapper sessions are `incremental=fresh` and close native APIs after use.

Negative twins discriminate: repeated diagnosticDirectives, unused Expect not reported in the original file, Alias-as-kebab/Pascal rename, query-file snapshot reuse for a foreign definition, Verter-only postprocessor claimed as stock CLI, 7.1.0-dev version-label capability, dormant TCM2 complete-capability claims, silent-degrade neither-status rows, and blocking rows with empty defect evidence.

AC2 edit application remains with the LSO/provider snapshot authority named in `tests/sfc-projection/STP5/products/observation-role-policy.json`.

This file does not claim the cases executed. Copying the charter here is not evidence.
