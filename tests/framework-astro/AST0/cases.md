# AST0 planted-row cases

Every case below is a planted mutation of one AST0 product. A correct
validator rejects it with the stated reason. AST0 lands no validator: the
`node --test tests/framework-astro/AST0/astro-lock.spec.ts` proof and its
`contract.ts` belong to AST1G (`AST1G-ACV`), and REG0's
`scripts/run-framework-locks.mjs` discovers the spec. A planted row counts
only once the plant is proven present, unique and new in the product it
mutates.

Products:

- `products/astro-version-lock.json` (`V`)
- `products/astro-capability-matrix.json` (`M`)
- `products/astro-activation-policy.json` (`A`)

The clean twin of every case is the unmodified product set, which the
validator accepts.

## AST0-AC1 — pinned release

| Twin | Plant | Expected rejection |
| --- | --- | --- |
| `floating-range-pin` | `V#/admittedReleases/0/version` = `^7.3.8` | not an exact version (VID0 R07) |
| `latest-tag-pin` | `V#/admittedReleases/0/version` = `latest` | floating tag never decodes into a release (R07) |
| `beta-channel-admitted-as-second-profile` | append `7.4.0-beta.1` to `V#/admittedReleases` | a prerelease minor of the admitted major is not a next major; two releases in one manifest (R04) |
| `unknown-version-pin` | `V#/admittedReleases/0/version` = `7.3.99` | not a published release of the named source |
| `missing-official-source` | delete `V#/admittedReleases/0/source` | every pin names an official source |
| `two-releases-in-one-manifest` | `V#/admittedReleases/0/version` = `["7.3.7", "7.3.8"]` | one manifest declares exactly one release (R04) |
| `legacy-major-profile` | `V#/admittedReleases/0/version` = `6.4.8` | Astro 5 and 6 are not profiles |
| `wdx1-astro-5-0-0-diverged-pin` | the live `tests/web-product/WDX1/fixtures/mixed-framework/case.json` row `astro@5.0.0` (already present at the described head) | diverged pin: the validator reports it until AST9 re-pins the scenario; deleting it from `V#/divergedPins` while the fixture still pins 5.0.0 also fails |
| `oracle-pin-floating` | `V#/toolingOracles/pins/0/version` = `^0.5.1`, or the `@astrojs/astro2tsx` pin's version = `^0.1.0` | oracle pins are exact; the upstream declared ranges are provenance only |
| `obsolete-grammar-oracle` | replace the `@astrojs/compiler-rs` pin with `@astrojs/compiler` 4.0.0 | the grammar oracle must match the ratified Astro 7 oracle selection |
| `missing-projection-oracle` | remove the `@astrojs/astro2tsx` pin | the separate projection oracle must be pinned at 0.1.2 |
| `missing-oracle-integrity` | delete `integrity` from either replacement oracle pin | each selected oracle records its published package integrity |
| `oracle-as-product-dependency` | an oracle `role` reads `parser` or the oracle is listed as a product dependency | oracles are test-only, never product authority |

## AST0-AC2 — owned matrix

| Twin | Plant | Expected rejection |
| --- | --- | --- |
| `unowned-cell` | delete `producer` from cell `C02` | an owned cell names one producer and one receiving acceptance ID |
| `duplicate-owner-cell` | cell `C02` `producer` = `["AST5", "AST6"]` | exactly one producer per cell |
| `duplicate-operation-host-profile-cell` | copy cell `C14` with a new id | one cell per operation × host × profile |
| `unknown-acceptance-id` | cell `C14` `receivingAcceptance` = `AST1S-AC9` | the acceptance ID does not exist in the producer's charter |
| `bnd-build-consumption-cell` | cell `C52` `disposition` = `owned`, `producer` = `AST6` | official build output is never consumed (ruling 4) |
| `runtime-execution-cell` | cell `C54` `disposition` = `owned` | no runtime cell (framework-common) |
| `fabricated-feature-cell` | add an owned cell `astro dev-server preview` or `Astro.request mock evaluation` | the feature has no producer outcome in the plan |
| `exclusion-without-reason` | delete `reason` from cell `C10` | an exclusion is truthful only with its reason |
| `cell-on-excluded-profile` | any cell with `profile` = `astro-7.4.0-beta.1` | the beta profile is excluded and carries no cells |

## AST0-AC3 — activation and host policy

| Twin | Plant | Expected rejection |
| --- | --- | --- |
| `extension-only-activation` | `A#/activationRow/dependencyNames` = `[]` with a rule activating on `.astro` | the extension never activates (`A-X01`) |
| `second-activation-source` | `A#/activationSource/only` lists FWA1 and an LSP-local probe | FWA1 is the only activation source (`A-X02`) |
| `declared-range-activation` | `A#/activationRow` admits the declared `package.json` range | activation reads the resolved installed version (`A-X03`, R08) |
| `verter-native-ts-region-cell` | cell `C02` `host` = `verter-lsp` | a Verter-native answer to a TS-region operation duplicates tsgo (`C12`) |
| `tsserver-specific-cell` | cell `C35` `notes` gains Astro-specific plugin behaviour, or cell `C36` becomes owned | tsserver receives carrier-generic compatibility only |
| `astro-compiler-parser-authority` | `M#/parserDecision/carrier/decision` = `Reuse` of `@astrojs/compiler` | upstream implementations are oracles only (PAR0 PD04) |
| `astro-compiler-rs-parser-authority` | `M#/parserDecision/carrier/decision` = `Reuse` of `@astrojs/compiler-rs` | the grammar oracle never becomes the production parser (PAR0 PD04) |
| `astro2tsx-projection-authority` | `M#/hosts/rows/0/projection` = `@astrojs/astro2tsx output` | AST6 owns the production projection; upstream TSX is test-only comparison data |
| `new-client-transport` | `M#/hosts/rows/0/transport` names a new client transport | rescope trigger: decision 4 uses the existing tsgo routes |

## AST0-AC4 — proof is not support

| Twin | Plant | Expected rejection |
| --- | --- | --- |
| `cell-cites-astp` | any cell gains `evidence: "ASTP"` | an architecture proof never satisfies a product claim |
| `cell-cites-installed-parser` | any cell gains `evidence: "@astrojs/compiler-rs installed"` or `evidence: "@astrojs/astro2tsx installed"` | an installed oracle never proves product support |
| `cell-cites-syntax-highlighting` | any cell gains `evidence: "AST1G TextMate grammar"` | syntax highlighting is never product evidence |
| `claim-basis-above-none-at-ratification` | any cell `claimBasis` = `runtime-observation` | no cell is claimed at ratification; claims come from AST9 and AST10 |

## AST0-AC5 — wire tag ratified

| Twin | Plant | Expected rejection |
| --- | --- | --- |
| `open-canonical-adapter-tag` | `V#/wireTag/value` = `5` | `OPEN_CANONICAL` is a structural non-tag (`tag_disposition` returns `None`) |
| `reused-tag-value` | `V#/wireTag/value` = `2` | the value is already `FRAMEWORK_TAG_SVELTE` |
| `class-a-collision` | `V#/wireTag/value` = `7` | 6–9 is the class-A allocation |
| `missing-tag` | delete `V#/wireTag` | the lock records `FRAMEWORK_TAG_ASTRO = 10` |
