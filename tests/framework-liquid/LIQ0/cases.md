# LIQ0 cases

The Liquid lock's acceptance cases. Each case has a clean leg (the products as
committed pass) and planted rows. A planted row is one edit to one product; the
validator must reject it for the reason given, and only for that reason.

No validator ships with this lock. LIQ1G adds
`tests/framework-liquid/LIQ0/contract.ts` and
`tests/framework-liquid/LIQ0/liquid-lock.spec.ts`, which implement every row
below against `products/*.json` (`LIQ1G-ACV`). The command
`node --test tests/framework-liquid/LIQ0/liquid-lock.spec.ts` belongs to that
node. This file does not claim any case has run.

Products: `V` = `liquid-version-lock.json`, `W` = `liquid-vocabulary.json`,
`M` = `liquid-capability-matrix.json`, `A` = `liquid-activation-policy.json`.

## LIQ0-AC1 — pinned profiles

Clean leg: each of the four profiles has one release with an exact
`referencePin`, a registry, an integrity and an `admittedLine`; every vocabulary
table names one source that is a pinned artifact of its own dialect.

| Twin | Planted row | Expected failure |
| ---- | ----------- | ---------------- |
| `floating-range-pin` | `V` liquidjs `referencePin` = `^10.30.0` | floating pin: a range is not an exact version (VID0 R07) |
| `dist-tag-pin` | `V` eleventy `referencePin` = `latest` | floating pin: a dist-tag never decodes into a release (VID0 R07) |
| `canary-line-pin` | `V` eleventy `referencePin` = `4.0.0-alpha.10` | canary line: the version is published under the `canary` tag and is not admitted |
| `nightly-pin` | `V` liquidjs `referencePin` = a `-nightly` or `-dev` build | nightly line: never admitted |
| `legacy-major-pin` | `V` shopify `release.referencePin` = `4.0.4` with `admittedLine` `4` | legacy major: the shopify profile admits the `liquid` 5 line only |
| `pin-without-source` | `V` jekyll `release.integrity` or `registry` removed | pin without a named source and integrity |
| `diverged-engine-pin` | `V` eleventy `engine.pin` = `10.29.0` while liquidjs `referencePin` stays `10.30.0` | diverged pin: the Eleventy engine pin must equal the liquidjs profile's reference pin |
| `shared-entry-without-own-source` | `W` add `post_url` to a shopify tag table whose source is the jekyll gem, or delete the jekyll `engine-core` table while `include` stays in the jekyll table | shared vocabulary entry without a source of its own dialect |
| `unratified-next-line-admitted` | `V` `nextLines` liquidjs line 11 `disposition` = `admitted` | a prerelease next line becomes a profile only through a ratified amendment of this lock |
| `two-releases-in-one-profile` | `V` liquidjs `release.referencePin` = `["10.29.0", "10.30.0"]` | one profile, one release (VID0 R04) |
| `table-source-outside-pinned-artifacts` | `W` a liquidjs filter table whose `source.pin` is `10.31.0` | the table's source is not an artifact pinned in `V` for that dialect |

Not a failure: the jekyll profile's `engine` is `liquid` 4.0.4. It is the
runtime dependency of the admitted Jekyll release, not an admitted `liquid`
line, so `legacy-major-pin` does not apply to it. A validator that rejects the
clean products for it is wrong.

## LIQ0-AC2 — owned matrix

Clean leg: every `M` cell has one `producer`, one `acceptance`, an empty
`tsgoOperations`, and for each of the four dialects either `admitted` or an
exclusion with a non-empty reason. `facets` are all `absent`; `frameworkTag` is
`null`. A cell whose acceptance is not its producer's own item names
`LIQ9-AC1` and carries an `acceptanceNote`.

| Twin | Planted row | Expected failure |
| ---- | ----------- | ---------------- |
| `unowned-cell` | `M` a cell with `producer` removed or empty | unowned cell |
| `cell-without-acceptance` | `M` a cell with `acceptance` removed | cell without a receiving acceptance |
| `acceptance-of-another-producer` | `M` LQM33 (LIQ6) with `acceptance` = `LIQ5-AC1` | the acceptance is neither the producer's nor `LIQ9-AC1` with a note |
| `render-output-cell` | `M` add a cell "preview rendered HTML" with any producer | render-output cell: no template is rendered (exclusion LQE1) |
| `build-cell` | `M` add a cell "run jekyll build" or "shopify theme dev" | build or dev-server cell (exclusion LQE2) |
| `ts-plugin-cell` | `M` a cell with `host` = `typescript-plugin` | TS plugin host (exclusion LQE3) |
| `tsgo-reprovide-cell` | `M` a cell with a non-empty `tsgoOperations` or a `tsgo` host | tsgo/TS re-provision (exclusion LQE4) |
| `snippet-componentinfo-facet` | `M` `facets.props` = `authoritative`, sourced from LiquidDoc | ComponentInfo facet for snippets (exclusion LQE5) |
| `framework-tag-present` | `M` `frameworkTag.value` = any wire tag | Liquid has no `FrameworkTag` |
| `exclusion-without-reason` | `M` LQM17 jekyll = `{ "excluded": "" }` | exclusion without a truthful reason |
| `grammar-colouring-cell` | `M` add a cell whose producer is LIQ1G | colouring is never product evidence (LIQ1G-AC6) |
| `unknown-dialect-column` | `M` a cell with a fifth dialect key (for example `liquidjs11`) | dialect outside the admitted profiles |

## LIQ0-AC3 — activation row

Clean leg: `A` activates only through AP-S1 (both theme-root files), AP-S2
(`jekyll` in `Gemfile.lock`), AP-S3 (`@11ty/eleventy` resolved), AP-S4
(`liquidjs` resolved directly, no Eleventy) or AP-S5 (explicit `on` naming an
admitted release); every mode is one profile of `V`; NeedSelection is resolved
only by AP-S5.

| Twin | Planted row | Expected failure |
| ---- | ----------- | ---------------- |
| `extension-activation` | `A` a source whose evidence is "a `.liquid` file" | activation from a file extension |
| `cdn-script-activation` | `A` a source whose evidence is a `<script src>` naming a Liquid build | activation from a CDN script |
| `single-theme-root-file` | `A` AP-S1 evidence reduced to `layout/theme.liquid` alone | directory evidence other than the two theme-root files together |
| `other-directory-evidence` | `A` a source activating `jekyll` from `_config.yml`, or `shopify` from `snippets/` | directory evidence other than the two theme-root files |
| `dialect-without-resolved-version` | `A` AP-S2 `admittedRelease` = "the jekyll reference pin" when no version resolves | dialect chosen without a resolved version or a named `on` profile |
| `explicit-on-without-profile` | `A` AP-S5 evidence `frameworks.liquid = on` with no release | explicit `on` that names no profile |
| `bare-liquid-gem-activation` | `A` a source activating any mode from `liquid` alone in `Gemfile.lock` | the lockfile source is `jekyll`; a bare engine names no dialect |
| `need-selection-by-guess` | `A` `association.needSelection` resolved by nesting depth or by the nearer claim | NeedSelection is never a guess (VID0 R10) |

Not a failure: AP-S1 admits the shopify reference release with no resolved
version. A theme vendors no engine; the theme root is the charter's named
source for that dialect.

## LIQ0-AC4 — coexistence cells

Clean leg: for every competitor in `M.competitors` and every capability it
`provides` that some `M` cell also has (as `capability` or in
`alsoCapabilities`; `cli.lint` matches a `textDocument/publishDiagnostics` cell
whose `host` or `alsoHosts` includes `cli`), there is exactly
one `M.coexistence` row naming that competitor and capability, and its `cells`
are exactly those cells. i18n Ally rows cover only LIQ11/LIQ11T cells.

| Twin | Planted row | Expected failure |
| ---- | ----------- | ---------------- |
| `missing-coexistence-cell` | `M` delete LQX06 (Shopify Liquid, `textDocument/rename`) | a capability the competitor also provides has no COXD1 cell |
| `coexistence-cell-without-competitor` | `M` LQX10 `competitor` removed | a coexistence cell must name its competitor |
| `capability-not-provided-by-competitor` | `M` add a row for `shopify-liquid-vscode` on `textDocument/semanticTokens` | the competitor does not provide that capability |
| `i18n-ally-outside-translation-cells` | `M` LQX15 `cells` += LQM50 | i18n Ally cells are the LIQ11 translation cells only |
| `dangling-matrix-cell-reference` | `M` LQX01 `cells` += `LQM99` | the row references a cell that does not exist |
