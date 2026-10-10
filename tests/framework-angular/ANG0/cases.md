# ANG0 planted-row cases

These are the cases the Angular lock validator must decide. ANG0 lands the
products and this table only. The validator
(`tests/framework-angular/ANG0/contract.ts` and `angular-lock.spec.ts`) is
ANG1G's (`ANG1G-ACV`), and `scripts/run-framework-locks.mjs` (`REG0-AC6`) runs
it.

Each case starts from the clean products in `products/`, applies the
planted change named in its row, and expects the result shown. A `reject` row
must include the recorded reason in its set of rejection reasons. Other
independently applicable reasons are permitted: tag collisions can also cause
allocation mismatches, and invalid pins can also diverge from their peers.
The validator collects applicable reasons without a first-error precedence.
A plant that does not apply fails the case; it never counts as a pass.
Modify/delete plants require their target row (and any named target field)
to exist, and a modification must change its value. Add-field plants require
the target row or named clone source to exist, with the field absent before
planting and newly present with the stated value afterward. Add-row plants
require a newly present row; clone plants also require their named source.

Reasons describe structural contract violations, never differences in the
prose of an operation label. AC2-P12 and AC3-P12 intentionally exercise the
same forbidden profile from the matrix and policy acceptances and require the
same reason. Added-cell plants clone the complete named cell, give it a fresh
unique ID, and apply only the stated changes; unspecified fields stay intact.

`unknown-version` means an exact package version lacks a matching reviewed
package/version/source/integrity tuple in ANG1G's offline pin expectations.
It does not assert that a version was never published. Those expectations are
derived from the reviewed lock when the validator lands and change only with
an explicit release re-pin; tests mutate the input, not those expectations.
No registry lookup or comparison of commit identities is part of a lock test.

ANG1G's offline required-cell expectations likewise remain independent of
the matrix being checked and of every planted input. They encode the ratified
ANG5-AC5 completion operations at both inline and external locations
(`ANG-C38` TS-region completion and `ANG-C41` carrier-only completion), and
ANG5-AC6's required inline operations: syntax diagnostics (`ANG-C59`), document
symbols (`ANG-C60`), folding/selection ranges (`ANG-C61`), carrier-only semantic
tokens (`ANG-C62`), and gating/incremental/cancellation/parse reuse (`ANG-C63`).
Match these requirements structurally by operation and location, not operation
label wording. Deleting a cell or moving its operation/location into exclusions
does not remove the requirement. Only a reviewed delivery-contract amendment
changes these expectations; tests mutate the products, never the expectations.

## Clean products

| Id | Plant | Expected |
| -- | ----- | -------- |
| `ANG0-CLEAN` | none | accept: every product parses, every cross-reference below resolves, and no rejection reason fires |

## ANG0-AC1 — pinned release

Target: `products/angular-version-lock.json`.

| Id | Plant | Expected | Reason |
| -- | ----- | -------- | ------ |
| `AC1-P01` | `@angular/core` version `^22.2.1` | reject | `floating-range` |
| `AC1-P02` | `@angular/core` version `latest` | reject | `floating-tag` |
| `AC1-P03` | `@angular/compiler-cli` version `next` | reject | `floating-tag` |
| `AC1-P04` | `@angular/core` version `22.x` | reject | `floating-range` |
| `AC1-P05` | `@angular/core` version `22.9.9`, leaving its reviewed 22.2.1 source and integrity intact | reject | `unknown-version` |
| `AC1-P06` | `@angular/core` and `@angular/compiler-cli` versions differ (`22.2.1` and `22.2.0`) | reject | `diverged-pin` |
| `AC1-P07` | admitted release `angular@20.0.0`, the WDX1 `mixed-framework` pin | reject | `diverged-pin` |
| `AC1-P08` | an admitted package row cites `tests/web-product/WDX1/fixtures/mixed-framework/case.json` as its source | reject | `diverged-pin` |
| `AC1-P09` | `source` removed from `@angular/core` | reject | `missing-source` |
| `AC1-P10` | `integrity` removed from `@angular/core` | reject | `missing-integrity` |
| `AC1-P11` | a second admitted release `angular@22.2.2` beside `angular@22.2.1` | reject | `two-releases-in-one-manifest` |
| `AC1-P12` | `angular-23-next` moved to `admittedReleases` with version `23.0.0-next` | reject | `unknown-version` |
| `AC1-P13` | admitted release `angular@21.2.25` | reject | `legacy-major` |
| `AC1-P14` | admitted release with channel `canary` | reject | `canary-or-nightly` |
| `AC1-P15` | `angular-23-next` exclusion row deleted | reject | `missing-exclusion-reason` |
| `AC1-P16` | the WDX1 row removed from `divergedPins` | reject | `diverged-pin-uninventoried` |

## ANG0-AC2 — owned matrix

Target: `products/angular-capability-matrix.json`.

| Id | Plant | Expected | Reason |
| -- | ----- | -------- | ------ |
| `AC2-P01` | `producer` removed from `ANG-C12` | reject | `unowned-cell` |
| `AC2-P02` | `receivingAcceptance` removed from `ANG-C21` | reject | `unowned-cell` |
| `AC2-P03` | `ANG-C21` producer becomes `["ANG3", "ANG6"]` | reject | `duplicate-owner` |
| `AC2-P04` | clone `ANG-C30` with the same operation and locations, producer `ANG5` and receiving acceptance `ANG5-AC1` | reject | `duplicate-owner` |
| `AC2-P05` | `ANG-C55` receiving acceptance becomes `ANG6-AC1`, which its producer does not own | reject | `acceptance-not-owned-by-producer` |
| `AC2-P06` | `ANG-C21` receiving acceptance becomes `ANG3-AC9`, which no charter defines | reject | `unknown-acceptance` |
| `AC2-P07` | clone `ANG-C30` with `buildConsumption: "ng-build-output"` | reject | `build-cell` |
| `AC2-P08` | clone `ANG-C30` with `buildMapPromise: "DBG"` | reject | `build-cell` |
| `AC2-P09` | clone `ANG-C30` with `runtimeExecution: "dev-server"` | reject | `runtime-cell` |
| `AC2-P10` | clone `ANG-C03` with `supportedStructuralSugar: ["*ngIf"]` | reject | `excluded-sugar-supported` |
| `AC2-P11` | `ANG-X05` exclusion deleted | reject | `missing-exclusion` |
| `AC2-P12` | clone `ANG-C20` with `compilationScope: "ngmodule-declarations"` and `scopeDisposition: "supported"` | reject | `ngmodule-scope-supported` |
| `AC2-P13` | `ANG-X04` exclusion deleted | reject | `missing-exclusion` |
| `AC2-P14` | an exclusion row with no `reason` | reject | `missing-exclusion-reason` |
| `AC2-P15` | the expression-grammar parser decision deleted | reject | `missing-parser-decision` |
| `AC2-P16` | the template-markup parser decision becomes `ForkAndSpecialize` with `home` `H08 crates/verter_html_syntax` | reject | `angular-branch-in-neutral-parser` |
| `AC2-P17` | a facet row `slots` with provenance `native` | reject | `facet-provenance-mismatch` |
| `AC2-P18` | facet `expose` deleted | reject | `unmapped-facet` |
| `AC2-P19` | `ANG-C38` receiving acceptance becomes `ANG9-AC1` | reject | `acceptance-not-owned-by-producer` |
| `AC2-P20` | `ANG-C41` receiving acceptance becomes `ANG5-AC1`, which does not cover completion | reject | `acceptance-does-not-cover-operation` |
| `AC2-P21` | inline syntax-diagnostics cell `ANG-C59` removed | reject | `missing-required-cell` |
| `AC2-P22` | `ANG-C60` producer becomes `ANG1S` and receiving acceptance becomes `ANG1S-AC2`, whose delivery scope is external only | reject | `acceptance-does-not-cover-location` |
| `AC2-P23` | `ANG-C63` removed, leaving inline structure without gating/incremental/cancellation coverage | reject | `missing-required-cell` |
| `AC2-P24` | move `ANG-C59`, `ANG-C60`, `ANG-C61`, `ANG-C62` and `ANG-C63` from admitted cells into exclusions preserving their inline operations and locations | reject | `required-cell-excluded` |

## ANG0-AC3 — activation and host policy

Targets: `products/angular-activation-policy.json` and
`products/angular-capability-matrix.json`.

| Id | Plant | Expected | Reason |
| -- | ----- | -------- | ------ |
| `AC3-P01` | `htmlNeutrality.rule` replaced by "every `.html` file in an Angular package is an Angular template" | reject | `generic-html-activation` |
| `AC3-P02` | `activationSource.owner` becomes `LK6` | reject | `second-activation-source` |
| `AC3-P03` | a second activation source `LK6 lint.angular switch` added beside FWA1 | reject | `second-activation-source` |
| `AC3-P04` | `detection.reads` becomes "the presence of angular.json" | reject | `second-activation-source` |
| `AC3-P05` | `detection.reads` becomes "the declared dependency range" | reject | `declared-range-activation` |
| `AC3-P06` | `hostPolicy.angularLanguageService` becomes `authority` | reject | `angular-ls-authority` |
| `AC3-P07` | clone `ANG-C36` with host `angular-language-service` | reject | `angular-ls-authority` |
| `AC3-P08` | `ANG-C36` host becomes `lsp-enhancement` (Verter computes the TS-region hover) | reject | `verter-computed-ts-answer` |
| `AC3-P09` | `ANG-C38` host becomes `lsp-enhancement` | reject | `verter-computed-ts-answer` |
| `AC3-P10` | clone `ANG-C35` with host `typescript-plugin-angular` | reject | `angular-specific-ts-plugin-row` |
| `AC3-P11` | `ANG-C35` `angularSpecific` becomes `true` | reject | `angular-specific-ts-plugin-row` |
| `AC3-P12` | clone `ANG-C20` with `compilationScope: "ngmodule-declarations"` and `scopeDisposition: "supported"` (same structural plant as AC2-P12) | reject | `ngmodule-scope-supported` |
| `AC3-P13` | `hostPolicy.order` lists `tsgo-managed` before `tsgo-shared` | reject | `host-order` |
| `AC3-P14` | `hostPolicy.externalTemplates` loses its FWA1 gate | reject | `ungated-external-template-row` |
| `AC3-P15` | `inactiveMeansZeroWork.noWorkOf` drops `format` | reject | `inactive-work` |
| `AC3-P16` | `switch.on` becomes "admits any installed version" | reject | `on-admits-unadmitted-release` |
| `AC3-P17` | `hostPolicy.inlineTemplates` loses its LSPX11 seam or ANG1M authored-map requirement | reject | `unmapped-inline-structure-route` |
| `AC3-P18` | `ANG-C62` gains `tokensInTsRegions: true` | reject | `verter-computed-ts-answer` |

## ANG0-AC4 — proof is not support

Target: `products/angular-capability-matrix.json`.

| Id | Plant | Expected | Reason |
| -- | ----- | -------- | ------ |
| `AC4-P01` | `ANG-C16` gains `evidence: "ANGP architecture proof"` | reject | `proof-cited-as-support` |
| `AC4-P02` | `ANG-C01` gains `evidence: "@angular/compiler-cli installed in node_modules"` | reject | `installed-compiler-cited-as-support` |
| `AC4-P03` | `ANG-C14` gains `evidence: "ANG1G TextMate grammar"` | reject | `highlighting-cited-as-support` |
| `AC4-P04` | `ANG-C01` producer becomes `ANGP` | reject | `proof-cited-as-support` |
| `AC4-P05` | `ANG-C01` producer becomes `ANG1G` | reject | `highlighting-cited-as-support` |

## ANG0-AC5 — wire tag ratified

Target: `products/angular-version-lock.json#wireTag`.

| Id | Plant | Expected | Reason |
| -- | ----- | -------- | ------ |
| `AC5-P01` | `wireTag` removed | reject | `missing-wire-tag` |
| `AC5-P02` | `value` becomes `5` with name `FRAMEWORK_TAG_OPEN_CANONICAL` | reject | `open-canonical-adapter-tag` |
| `AC5-P03` | `value` becomes `3` (React) | reject | `reused-tag-value` |
| `AC5-P04` | `value` becomes `10` (Astro) | reject | `reused-tag-value` |
| `AC5-P05` | `value` becomes `7` | reject | `class-a-collision` |
| `AC5-P06` | `value` becomes `15` | reject | `tag-not-ratified-value` |
| `AC5-P07` | `allocation.classB.ANGULAR` becomes `12` while `value` stays `11` | reject | `tag-allocation-mismatch` |
