# GLM0 evidence cases

These are the reviewed cases of the Ember/Glimmer delivery and exact-version
contract. The contract lands as data and text only: no validator, spec, CI
gate or `package.json` script is part of it. The executable lock validator
(`contract.ts` and `glimmer-lock.spec.ts`, run by
`node --test tests/framework-ember-glimmer/GLM0/glimmer-lock.spec.ts` and
discovered by REG0's `scripts/run-framework-locks.mjs`) is owned by GLM1G
(`GLM1G-ACV`). Every planted row below is the input that validator must
reject, with the failure reason it must report. Until it lands, each
acceptance is met by the reviewed products in `products/` and by this table.

The products describe the repository at `docs(arch): define the compiler
request, policy and stage identity (#819)`, 2026-10-09. Registry facts were
read from the npm registry on 2026-10-09.

| Product | File |
| ------- | ---- |
| `GlimmerVersionLock` | `products/glimmer-version-lock.json` |
| `GlimmerCapabilityMatrix` | `products/glimmer-capability-matrix.json` |
| `GlimmerActivationPolicy` | `products/glimmer-activation-policy.json` |

## GLM0-policy-lock (accept)

The clean products validate:

- every version in the lock is one exact published version with an official
  source;
- every row × profile cell of the matrix names exactly one producer node and
  one receiving acceptance ID, or a truthful exclusion with a reason;
- the activation policy names FWA1 as its only source;
- the wire tag is `FRAMEWORK_TAG_EMBER_GLIMMER = 13`.

## GLM0-AC1 — pinned release (reject)

| Planted row | Expected failure |
| ----------- | ---------------- |
| `framework.admittedReleases[0].version = "^6.12.0"` | `floating-version-range` |
| `framework.admittedReleases[0].version = "latest"` | `dist-tag-pin` |
| `framework.admittedReleases[0].version = "6.x"` | `floating-version-range` |
| `framework.admittedReleases[0].version = "6.99.0"` | `unknown-version`: not a published `ember-source` release |
| an admitted `7.5.0-alpha.3` or any `-alpha.N` release | `canary-excluded` |
| an admitted `7.4.0-beta.1` while decision 1 excludes the beta | `beta-excluded` |
| an admitted `5.12.0` | `legacy-major` |
| two admitted 6.x minors (`6.11.1` and `6.12.0`) | `one-release-per-manifest` (VID0) |
| an oracle `ember-template-lint` pinned `7.9.4` while LEM1 locks `7.9.3` | `diverged-pin` |
| `@glint/template` pinned to a version other than the exact one `@glint/ember-tsc 1.11.6` depends on (`1.9.0`) | `diverged-pin` |
| an oracle row with no `officialSource` | `provenance-source-missing` |
| an oracle with `role: "production-parser"` (`@glimmer/syntax` or `content-tag`) | `oracle-as-production` (PAR0 CL14) |

## GLM0-AC2 — owned matrix (reject)

| Planted row | Expected failure |
| ----------- | ---------------- |
| a cell with no `producer` and no `exclusion` (drop `ts-hover`.`strict-gts`) | `unowned-cell` |
| a profile missing from a row's `cells` | `missing-cell` |
| a cell naming two producers, or the same `op` × profile owned by two rows | `duplicate-owner` |
| a `producer` outside the `GLM*` nodes, or an `acceptance` not of the form `<producer>-AC<n>` | `malformed-owner` |
| a row `{ op: "embroider-build-map", host: "verter-substrate" }` consuming ember-cli/Embroider build output | `build-output-cell` (ruling 4) |
| a row promising a DBG, TST or WPF map | `build-output-cell` (ruling 4) |
| a row that renders, hydrates or runs Ember code | `runtime-cell` (framework-common) |
| a row for a feature no GLM charter delivers (for example `ts-signature-help-carrier` on `lsp-enhancement`, or a `component-facet-model` row) | `fabricated-feature` |
| an exclusion with an empty `reason` | `exclusion-without-reason` |
| `component-facet-expose` with `outcome: "SUPPORTED"` or an empty surface | `expose-must-be-unsupported` |
| a parser decision row for `glimmer-template` with `decision` outside `Reuse`, `ForkAndSpecialize`, `NewParser` | `unknown-parser-decision` |
| the `.gjs/.gts` `script_ts()` displaced route removed from `displacedRoutes` | `displaced-route-not-inventoried` |

## GLM0-AC3 — modes and activation (reject)

| Planted row | Expected failure |
| ----------- | ---------------- |
| a profile `{ id: "classic-hbs", mode: "classic" }` admitting `@ember/component` | `classic-profile` |
| a profile admitting `{{action}}`, `{{mut}}`, classic classes, implicit-this fallback or pre-Octane resolution | `classic-profile` |
| an `unsupportedModes` entry removed (for example `implicit-this-fallback`) | `excluded-mode-unreported` |
| `activationAuthority.sole = "extension"` or an activation source keyed on `.gjs`/`.gts`/`.hbs` | `extension-only-activation` |
| an activation source keyed on `ember-cli-build.js` presence or a directory name | `name-based-activation` |
| a second activation source beside FWA1 (`"LK6"`, a lint-pack toggle, or a per-node predicate) | `second-activation-source` |
| a state that does work while `inactive`, `unsupported-version`, `off` or `unproven` | `work-while-inactive` |
| a `lsp-enhancement` row answering a TS-region operation (`ts-hover`, `ts-completion`, `ts-definition`, `ts-references`, `ts-diagnostics`) | `duplicate-ts-answer` |
| a host `tsserver` or `glint-language-server` on any cell other than `tsserver-plugin-compatibility` | `forbidden-host` |
| a `tsgo` row whose `route` is not the shared-first, managed-second `TsgoCompositeProvider` order | `route-order` |

## GLM0-AC4 — proof is not support (reject)

| Planted row | Expected failure |
| ----------- | ---------------- |
| a cell whose evidence is `@glimmer/syntax` or `content-tag` being installed | `parser-installed-as-evidence` |
| a cell whose evidence is Glint type-checking (`@glint/ember-tsc`) or the Glint tsserver plugin | `oracle-as-evidence` |
| a cell whose evidence is the GLM1G TextMate grammar or any syntax highlighting | `highlighting-as-evidence` |
| a cell whose `acceptance` names an architecture-proof node instead of the producer's own acceptance | `architecture-proof-as-evidence` |

## GLM0-AC5 — wire tag ratified (reject)

| Planted row | Expected failure |
| ----------- | ---------------- |
| `wireTag.name = "FRAMEWORK_TAG_OPEN_CANONICAL"` or `wireTag.value = 5` | `open-canonical-adapter-tag` |
| `wireTag.value = 0` | `structural-non-tag` |
| `wireTag.value` equal to a live tag (1-4) or another family's planned value (10, 11, 12, 14) | `reused-tag` |
| `wireTag.value` in the class-A range 6-9 | `class-a-collision` |
| `wireTag.value` other than 13 | `unratified-tag` |
| `wireTag.landsWith.node` other than GLM1 | `tag-lands-without-adapter` |

## Existing coverage cited (docs-only rule)

- Activation zero-work proofs are owned by `GLM1M-AC5`, `GLM1S-AC5` and
  `GLM8-AC1`; resolved-version admission by `FWA1-AC1`, `FWA1-AC3` and
  `FWA1-AC5`.
- The wire-tag disposition proof is `GLM1-AC6`. Today
  `FrameworkAdapterRegistry::tag_disposition` returns `None` for
  `FRAMEWORK_TAG_NONE` and `FRAMEWORK_TAG_OPEN_CANONICAL`, which the
  `framework_registry_complete` guard covers.
- The no-duplicate-TS-answer proof is `GLM5-AC1`; the generated-only
  suppression proof is `GLM6-AC2`.
- The parser lossless and recovery proofs are `GLM1-AC1` and `GLM1-AC2` (PAR0
  `PP04`, `CL14` grammar and recovery slots).
