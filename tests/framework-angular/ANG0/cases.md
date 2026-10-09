# ANG0 planted-row cases

These are the cases the Angular lock validator must decide. ANG0 lands the
products and this table only. The validator
(`tests/framework-angular/ANG0/contract.ts` and `angular-lock.spec.ts`) is
ANG1G's (`ANG1G-ACV`), and `scripts/run-framework-locks.mjs` (`REG0-AC6`) runs
it.

Each case starts from the clean products in `products/`, applies the one
planted change named in its row, and expects the result shown. A `reject` row
must fail for the recorded reason and no other. A plant that does not apply
(the target field or row is missing, or the planted value is already present)
fails the case; it never counts as a pass.

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
| `AC1-P05` | `@angular/core` version `22.9.9`, a version the registry never published | reject | `unknown-version` |
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
| `AC2-P04` | a second cell with the operation and locations of `ANG-C30` and producer `ANG5` | reject | `duplicate-owner` |
| `AC2-P05` | `ANG-C55` receiving acceptance becomes `ANG6-AC1`, an acceptance of neither its producer nor ANG9 | reject | `acceptance-not-owned-by-producer` |
| `AC2-P06` | receiving acceptance `ANG3-AC9`, which no charter defines | reject | `unknown-acceptance` |
| `AC2-P07` | a cell "consume ng build output and its maps" with producer `ANG6` | reject | `build-cell` |
| `AC2-P08` | a cell promising a DBG source map for Angular output | reject | `build-cell` |
| `AC2-P09` | a cell "render component in a dev server" | reject | `runtime-cell` |
| `AC2-P10` | a cell "`*ngIf` structural directive supported" with producer `ANG1` | reject | `excluded-sugar-supported` |
| `AC2-P11` | `ANG-X05` exclusion deleted | reject | `missing-exclusion` |
| `AC2-P12` | a cell "NgModule declarations scope binding" with producer `ANG2` | reject | `ngmodule-scope-supported` |
| `AC2-P13` | `ANG-X04` exclusion deleted | reject | `missing-exclusion` |
| `AC2-P14` | an exclusion row with no `reason` | reject | `missing-exclusion-reason` |
| `AC2-P15` | the expression-grammar parser decision deleted | reject | `missing-parser-decision` |
| `AC2-P16` | the template-markup parser decision becomes `ForkAndSpecialize` with `home` `H08 crates/verter_html_syntax` | reject | `angular-branch-in-neutral-parser` |
| `AC2-P17` | a facet row `slots` with provenance `native` | reject | `facet-provenance-mismatch` |
| `AC2-P18` | facet `expose` deleted | reject | `unmapped-facet` |

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
| `AC3-P07` | a cell with host `angular-language-service` | reject | `angular-ls-authority` |
| `AC3-P08` | `ANG-C36` host becomes `lsp-enhancement` (Verter computes the TS-region hover) | reject | `verter-computed-ts-answer` |
| `AC3-P09` | `ANG-C38` host becomes `lsp-enhancement` | reject | `verter-computed-ts-answer` |
| `AC3-P10` | a cell with host `typescript-plugin-angular` | reject | `angular-specific-ts-plugin-row` |
| `AC3-P11` | `ANG-C35` `angularSpecific` becomes `true` | reject | `angular-specific-ts-plugin-row` |
| `AC3-P12` | a cell "scope from `@NgModule` `declarations` array" with producer `ANG2` | reject | `declarations-scope-row` |
| `AC3-P13` | `hostPolicy.order` lists `tsgo-managed` before `tsgo-shared` | reject | `host-order` |
| `AC3-P14` | `hostPolicy.externalTemplates` loses its FWA1 gate | reject | `ungated-external-template-row` |
| `AC3-P15` | `inactiveMeansZeroWork.noWorkOf` drops `format` | reject | `inactive-work` |
| `AC3-P16` | `switch.on` becomes "admits any installed version" | reject | `on-admits-unadmitted-release` |

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
