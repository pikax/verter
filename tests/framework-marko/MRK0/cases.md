# MRK0 planted-row cases

Every row below is a mutation of the MRK0 products in `products/`. The
unmutated products must pass; each planted row must fail for the reason
given. MRK0 is docs-only and runs none of these: the validator
(`contract.ts`, `marko-lock.spec.ts`) and its proof
`node --test tests/framework-marko/MRK0/marko-lock.spec.ts` belong to MRK1G
(`MRK1G-ACV`), and REG0's `scripts/run-framework-locks.mjs` (`REG0-AC6`)
discovers the spec.

Abbreviations: **L** = `products/marko-version-lock.json`, **M** =
`products/marko-capability-matrix.json`, **A** =
`products/marko-activation-policy.json`.

## Clean twin

| Case | Input | Expected |
| ---- | ----- | -------- |
| `clean products` | L, M and A as committed | pass |

## MRK0-AC1 — pinned release

| Case | Planted row | Expected failure reason |
| ---- | ----------- | ----------------------- |
| `floating-range` | L `admittedReleases[0].exact = "^6.4.3"` | `release-not-exact`: a range never decodes into a release (VID0 R07) |
| `latest-tag` | L `admittedReleases[0].exact = "latest"` | `release-not-exact`: floating tag |
| `next-tag` | L adds an admitted release with `exact = "next"` | `release-not-exact`: floating tag; the next 6.x is admitted only by an exact manifest (XR02) |
| `implied-patch` | L `admittedReleases[0].exact = "6.4"` | `release-not-exact`: implied patch |
| `unpublished-version` | L `admittedReleases[0].exact = "6.4.99"` | `release-unknown`: the version is not in the official registry's published versions |
| `oracle-diverged-release` | L `admittedReleases[0].exact = "6.4.5"`, oracles unchanged | `release-diverged`: 6.4.5 declares `@marko/compiler ^5.42.11`, which excludes the oracle pin 5.42.10 (XR01) |
| `marko-5-pin` | L `admittedReleases[0].exact = "5.39.46"` | `release-excluded`: Marko 5 Class API (XR03) |
| `prerelease-pin` | L `admittedReleases[0].exact = "6.4.3-next.0"` | `release-not-exact`: pre-release (XR04) |
| `two-releases-one-manifest` | L `admittedReleases[0]` gains `"versions": ["6.4.3", "6.4.2"]` | `manifest-multi-release`: one manifest declares exactly one release (VID0 R04) |
| `missing-official-source` | L `admittedReleases[0].officialSources = []` | `source-missing`: a release names its official sources |
| `oracle-floating` | L `oracles[0].exact = "^5.42.10"` | `oracle-not-exact`: oracles are exact pins |

## MRK0-AC2 — owned matrix

| Case | Planted row | Expected failure reason |
| ---- | ----------- | ----------------------- |
| `unowned-cell` | M cell `C28` loses `producer` | `cell-unowned`: every cell names one producer |
| `duplicate-owner` | M adds a copy of `C28` with `producer = "MRK6"` | `cell-duplicate`: two cells claim one (operation, host, profile) |
| `unknown-receiving-acceptance` | M cell `C20` `receivingAcceptance = "MRK3-AC9"` | `acceptance-unknown`: the producer has no such acceptance id |
| `bnd-build-cell` | M adds a cell `{operation: "consume @marko/compiler build output", host: "build", producer: "MRK6"}` | `build-cell`: build output is an exclusion (X01), never a cell |
| `runtime-cell` | M adds a cell `{operation: "hydration timing", host: "runtime", producer: "MRK4"}` | `runtime-cell`: runtime is an exclusion (X04) |
| `fabricated-feature` | M adds a cell `{operation: "Marko Run route typing", host: "verter-kernel", producer: "MRK3", receivingAcceptance: "MRK3-AC1"}` | `feature-fabricated`: the receiving acceptance does not prove the operation; no MRK node owns route typing |
| `exclusion-without-reason` | M exclusion `X01` loses `reason` | `exclusion-unjustified`: an exclusion is truthful only with its reason |
| `borrowed-acceptance` | M cell `C32` or `C56` `receivingAcceptance = "MRK5-AC3"` | `feature-fabricated`: `MRK5-AC3` proves native/attribute-tag completion, not auto-close or custom-tag name completion; both require `MRK5-AC6` |
| `pending-without-proof` | M cell `C56` `receivingAcceptance = null`, with or without `pendingReceiver` | `cell-unowned`: every included cell requires a ratified receiving acceptance; an owed-proof note or open question cannot replace it |
| `tsgo-fields-unrecorded` | M cell `C11` loses `tsgoOperations` | `tsgo-unrecorded`: every cell records its tsgo operations, empty when none |
| `pending-cell-promoted` | M cell `C32` `receivingAcceptance = null` when evaluating `qualification.promotion` | `matrix-incomplete`: a cell with no ratified receiving acceptance cannot be promoted |
| `unqualified-cell-promoted` | M cell `C32` keeps `MRK5-AC6` but has neither an MRK9 pass nor a ratified exclusion when evaluating `qualification.promotion` | `matrix-incomplete`: a ratified receiving acceptance is not implementation or conformance evidence |

## MRK0-AC3 — activation and host policy

| Case | Planted row | Expected failure reason |
| ---- | ----------- | ----------------------- |
| `extension-only-activation` | A `activationRow.detection.packages = []` with a new field `extensions: [".marko"]` | `activation-by-extension`: the `.marko` extension never activates |
| `second-activation-source` | A adds `activationAuthority.secondary = "LSP package.json reader"` | `activation-second-source`: FWA1 is the only source |
| `declared-range-activation` | A `activationRow.detection.readFrom = "package.json dependencies range"` | `activation-declared-range`: activation reads the resolved installed version (VID0 R08) |
| `verter-computed-ts-answer` | M cell `C28` `host = "verter-lsp"` | `ts-region-duplicate`: a TS-region operation is answered by tsgo only |
| `tsserver-forwarding` | A `hostPolicy.tsserver.forwarding = "hover, completion"`, or M adds a cell with `host = "tsserver"` | `tsserver-forwarding`: nothing is forwarded to tsserver |
| `compiler-parser-authority` | A `parserAuthority.marko = "@marko/compiler"` | `oracle-as-authority`: `@marko/compiler` is an oracle only |
| `lk6-activation` | A `zeroWorkWhenInactive.lintPackSwitches = "LK6 activates marko"` | `activation-second-source`: LK6 keeps only the lint.marko pack switches |

## MRK0-AC4 — oracle is not support

| Case | Planted row | Expected failure reason |
| ---- | ----------- | ----------------------- |
| `language-tools-evidence` | M cell `C38` gains `evidence: "@marko/language-tools hover"` | `oracle-as-evidence` |
| `type-check-evidence` | M cell `C20` gains `evidence: "@marko/type-check output"` | `oracle-as-evidence` |
| `installed-parser-evidence` | M cell `C01` gains `evidence: "htmljs-parser parses the corpus"` | `oracle-as-evidence`: an installed parser is not product support |
| `syntax-highlighting-evidence` | M cell `C55` `supportClaim = "product"` | `highlighting-as-evidence`: a grammar never satisfies a product claim |

## MRK0-AC5 — wire tag ratified

| Case | Planted row | Expected failure reason |
| ---- | ----------- | ----------------------- |
| `open-canonical-tag` | L `wireTag.name = "FRAMEWORK_TAG_OPEN_CANONICAL"`, `value = 5` | `tag-non-adapter`: `OPEN_CANONICAL` is a structural non-tag |
| `reused-baseline-value` | L `wireTag.value = 2` | `tag-reused`: the value is already `FRAMEWORK_TAG_SVELTE` |
| `class-a-collision` | L `wireTag.value = 7` | `tag-class-a`: 6-9 belong to class A |
| `other-family-value` | L `wireTag.value = 13` | `tag-allocated`: 13 is EMBER_GLIMMER's |
| `missing-tag` | L loses `wireTag` | `tag-missing`: the lock records `FRAMEWORK_TAG_MARKO = 12` |
