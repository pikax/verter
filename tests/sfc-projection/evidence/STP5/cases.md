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

## Executed mapper host

Protocol: microsoft/typescript-go#4712 ("Content mappers") as amended by microsoft/TypeScript#63936 ("Content mappers round 2"); `microsoft/TypeScript` tag `v7.1-beta` ships the same `tsc/internal/contentmapper` host as `main` at the time of writing.

The host probe runs `tsc --help --all` on each selected engine (TypeScript 7.1 lists `--runExternalCode` only in the full help). Where it finds a host, `tests/sfc-projection/STP5/mapper-host/e2e.mjs` runs that engine's own `tsc --runExternalCode` against the fixture mapper (`tests/sfc-projection/STP5/mapper-host/fixture-mapper/`, spawned with the current Node; nothing is fetched) and records each host operation from what the run produced: UTF-8/UTF-16 position identity, `locale`, `diagnosticSource` (and its reserved-name refusal), `openProject` options and `optionDiagnostics`, `configIdentity` under `dynamicConfig` (required/forbidden, and `tsc -b` up-to-date until the dynamic config changes), `watchedFiles` (forbidden without `dynamicConfig`; a watched-file change re-opens the project under `--watch`, an unwatched one does not), `transform` with Verbatim/Alias/Atom spans reported at original positions, synthesized-region diagnostics, mapper parse diagnostics, `diagnosticDirectives` Ignore/Expect against a directive-free twin, the unused-Expect diagnostic and its index rule, required `extension`, and `closeProject`. Without a host every host operation is recorded as `no mapper host on <engine>@<version>`; the spec's end-to-end test reports itself skipped with that text, never passed.

Selected-build observation, 2026-10-22, Windows x64 (an observation record, not the proof; the proof re-executes on every run):

| Build | Host probe | Host operations |
| --- | --- | --- |
| ts-js `typescript@6.0.3` (pinned) | no host | none: `no mapper host on ts-js@6.0.3` |
| ts-native `typescript@7.0.2` (pinned) | no host | none: `no mapper host on ts-native@7.0.2` |
| ts-native `typescript@7.1.0-beta` (root pin swapped locally, not committed) | host | all 19 supported by the executed run |

With the root pin at 7.1.0-beta the STP1-style harness observation (hover, definition, references, edits, diagnostics through `typescript/unstable/sync`) cannot run: 7.1.0-beta exports the sync client as `typescript/sync`, and its snapshot API refuses the harness's update sequence ("Cannot update an inactive snapshot"). Those five rows stay blocked until the harness is ported with the oracle pin move. Mapper-mediated language-service features (hover/definition/references/rename through spans under `tsc --lsp`) are not exercised here.

This file does not claim the cases executed. Copying the charter here is not evidence.
