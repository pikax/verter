# MDX0 case table

Products: `products/mdx-version-lock.json`, `products/mdx-capability-matrix.json`,
`products/mdx-activation-policy.json`. Decision text: `docs/arch/framework-mdx.md`.

This node lands reviewed data and this table only. It lands no validator, no test and no CI
wiring. Each row below is a case the lock validator implements at MDX1G (`MDX1G-ACV`):
`node --test tests/framework-mdx/MDX0/mdx-lock.spec.ts` must accept the clean products, and each
planted row, applied alone to the clean products, must fail for the recorded reason. This file
does not claim any case executed.

Deletion population: empty. The contract is additive; no route is displaced.

## Clean products

| Case         | Planted change | Expected result |
| ------------ | -------------- | --------------- |
| `MDX0-clean` | none           | accepted        |

## MDX0-AC1 — pinned release

| Case                        | Planted change                                                                                                           | Expected failure reason                                                             |
| --------------------------- | ------------------------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------- |
| `AC1-floating-range`        | `@mdx-js/mdx` version `^3.1.1`                                                                                           | version is not an exact pin                                                         |
| `AC1-latest-tag`            | `@mdx-js/react` version `latest`                                                                                         | floating dist-tag                                                                   |
| `AC1-canary`                | `@mdx-js/mdx` and `@mdx-js/react` both `3.1.2-canary.0` (or both `3.2.0-rc.1`), sources and integrities renamed to match | prerelease/canary pin on the admitted 3.x line; only the prerelease rule rejects it |
| `AC1-legacy-major`          | `@mdx-js/mdx` version `2.3.0`                                                                                            | legacy major outside the admitted 3.x line                                          |
| `AC1-unratified-next-major` | `@mdx-js/mdx` version `4.0.0` with no MDX0 amendment                                                                     | next major not ratified                                                             |
| `AC1-diverged-pair`         | `@mdx-js/mdx` `3.1.1`, `@mdx-js/react` `3.0.1`                                                                           | MDX packages diverge within one release                                             |
| `AC1-missing-source`        | a package row without `source`                                                                                           | pin has no named official source                                                    |
| `AC1-second-react-lock`     | `reactProfile` replaced by an inline `react` version                                                                     | a second React lock; React is referenced from RCT0 only                             |
| `AC1-react-pin-differs`     | a React pin that differs from `tests/framework-react/RCT0/products/react-version-lock.json`                              | React pin diverges from RCT0 (assertion owned by `MDX1M-AC6`)                       |

## MDX0-AC2 — owned matrix

| Case                           | Planted change                                                                                                         | Expected failure reason                                                  |
| ------------------------------ | ---------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------ |
| `AC2-build-output-cell`        | cell `{operation: "compile-mdx", host: "verter-session", producer: "MDX6"}`                                            | build cell; build is exclusion `X01`                                     |
| `AC2-bnd-cell`                 | cell naming a BND entry or bundler hook for `.mdx`                                                                     | BND consumption is excluded                                              |
| `AC2-runtime-cell`             | cell `{operation: "render-mdx"}`                                                                                       | runtime cell; exclusion `X03`                                            |
| `AC2-unowned-row`              | a cell with no `producer`                                                                                              | unowned cell                                                             |
| `AC2-missing-acceptance`       | a cell with no `acceptance`                                                                                            | cell without a receiving AC                                              |
| `AC2-duplicate-owner`          | two cells with the same `operation`, `host` and `profile` and different producers                                      | duplicate owner                                                          |
| `AC2-unknown-producer`         | `producer: "MDX99"`                                                                                                    | producer is not a plan node                                              |
| `AC2-exclusion-without-reason` | an exclusion row with no `reason`                                                                                      | exclusion is not truthful                                                |
| `AC2-unknown-host`             | `host: "tsserver"`                                                                                                     | host outside the closed host set                                         |
| `AC2-second-markdown-parser`   | `parserDecisions` row giving Markdown `DK4 NewParser` in an MDX home                                                   | a second Markdown parser; MDX extends `CL32`                             |
| `AC2-planned-op-dropped`       | cell `C36` (MDX1S folding) removed with no exclusion naming it                                                         | a planned operation is neither cell nor exclusion                        |
| `AC2-grammar-cell`             | a cell with `producer: "MDX1G"` replacing exclusion `X12`                                                              | highlighting is not a product operation (`MDX1G-AC6`)                    |
| `AC2-capitalised-candidate`    | `C22.componentCandidateRule.rule` admitting a capitalised name as proof                                                | capitalised-name heuristic; only a proven React component is a candidate |
| `AC2-override-open`            | `overridableConstructs.markdown` gaining `div`, or a slot recorded for a name outside the list                         | the overridable set is closed at the pinned table of components          |
| `AC2-source-without-licence`   | a `featureSources.sources` row with no `licence`                                                                       | feature source not ratified with its licence                             |
| `AC2-obligation-unbound`       | `C20` back at `acceptance: "MDX9-AC2"`, or an `acceptanceObligation.acceptance` differing from its cell's `acceptance` | the discriminating case is not bound to its producer AC                  |

## MDX0-AC3 — activation and host

| Case                      | Planted change                                                                                             | Expected failure reason                             |
| ------------------------- | ---------------------------------------------------------------------------------------------------------- | --------------------------------------------------- |
| `AC3-extension-only`      | activation rule `active when file extension is .mdx`                                                       | extension-only activation (file-name rule)          |
| `AC3-preact-source`       | `A3` accepting `jsxImportSource: "preact"`                                                                 | Preact `jsxImportSource` is not MDX-with-React      |
| `AC3-vue-source`          | `A3` accepting `jsxImportSource: "vue"`                                                                    | Vue `jsxImportSource` is not MDX-with-React         |
| `AC3-markdown-only`       | `A1` dropped, so a Markdown-only project activates                                                         | activation without a resolved MDX release           |
| `AC3-declared-range`      | `A1` reading the declared range instead of the resolved version                                            | activation must read the resolved installed version |
| `AC3-evaluated-config`    | `A3`/`A4` sourced from executing a bundler config                                                          | a value that needs evaluation is `unknown`          |
| `AC3-verter-ts-answer`    | cell `C18` with `verterComputesTsAnswer: true`                                                             | Verter computes a TS answer; tsgo owns it           |
| `AC3-ts-plugin-work`      | cell `C33` with `tsPluginMdxWork: "decorate"`, or any cell with `host: "typescript-plugin"` doing MDX work | MDX work in the TS plugin                           |
| `AC3-tsserver-forwarding` | `hostPolicy.tsserverForwarding: true`                                                                      | tsgo has replaced tsserver                          |

## MDX0-AC4 — facets and tag

| Case                        | Planted change                                      | Expected failure reason                                     |
| --------------------------- | --------------------------------------------------- | ----------------------------------------------------------- |
| `AC4-complete-empty-events` | `facets.events` `{status: "complete", records: []}` | an unmapped facet is UNSUPPORTED, never empty               |
| `AC4-complete-empty-slots`  | `facets.slots` `{status: "complete", records: []}`  | same rule                                                   |
| `AC4-components-derived`    | `components` provenance `derived(...)`              | `components` is native from `@types/mdx` `MDXProps`         |
| `AC4-props-read-proven`     | `props.<name>` result `proven`                      | a props read is `derived(props-read)`, conditional          |
| `AC4-props-closed`          | `facets.props.open: false`                          | the props set is open (`MDXProps` index signature)          |
| `AC4-tag-value`             | `wireTag.value: 15`                                 | value differs from the framework-common allocation (MDX 14) |
| `AC4-tag-at-mdx1`           | `wireTag.landsWith` naming MDX1                     | the tag lands with the descriptor at MDX3                   |

## MDX0-AC5 — proof is not support

| Case                         | Planted change                              | Expected failure reason                      |
| ---------------------------- | ------------------------------------------- | -------------------------------------------- |
| `AC5-cites-mdxp`             | a cell `evidence: "MDXP"`                   | proof node cited as product evidence         |
| `AC5-cites-mdxr0`            | a cell `evidence: "MDXR0"`                  | proof node cited as product evidence         |
| `AC5-cites-stp7`             | a cell `evidence: "STP7"`                   | architecture proof cited as product evidence |
| `AC5-cites-installed-parser` | a cell `evidence: "@mdx-js/mdx installed"`  | an installed parser is not support           |
| `AC5-cites-highlighting`     | a cell `evidence: "MDX1G TextMate grammar"` | syntax highlighting is not support           |

## Successor fixture obligations

These are not MDX0 validator rows: they are the discriminating fixture cases each cell's
`acceptanceObligation` binds to its producer AC. The producer implements and executes them after MDX0.

| Cell  | AC       | Discriminating case                                                                                                                                                          |
| ----- | -------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `C20` | MDX6-AC2 | a type error inside an `.mdx` `{expression}` is published once by tsgo at its exact authored range; a planted off-by-one projection map fails                                |
| `C22` | MDX5-AC2 | a capitalised export that is not a proven React component is not a component auto-import candidate; a proven React component export is                                       |
| `C25` | MDX5-AC5 | the symbol tree holds each named ESM export and the layout default export under MDX1S's outline; a planted import symbol fails                                               |
| `C33` | MDX6-AC5 | a `.tsx` importer under the TS-plugin compat path resolves `import Doc from "./doc.mdx"` to the MDX3 surface; a planted MDX decoration in `packages/typescript-plugin` fails |
