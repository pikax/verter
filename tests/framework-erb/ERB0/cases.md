# ERB0 lock cases

The cases the ERB lock validator runs against `products/*.json`. Each planted
row is a mutation of one product; the validator must reject it for the reason
given. The validator itself (`contract.ts`, `erb-lock.spec.ts`) is ERB1G's
(`ERB1G-ACV`); this file is its specification. The clean products must pass.

Products: `erb-version-lock.json` (V), `erb-capability-matrix.json` (M),
`erb-activation-policy.json` (A), `erb-coexistence.json` (C).

## ERB0-AC1 — pinned profiles (`ERB0-pin`)

| Id | Product | Planted row | Expected failure |
| -- | ------- | ----------- | ---------------- |
| P01 | V | a release `pin` of `~> 8.1` | floating pin: not an exact version |
| P02 | V | a release `pin` of `latest` | floating tag is never a release |
| P03 | V | a release `pin` of `8.1.0.beta1` or `6.0.0.rc1` | prerelease is not admitted |
| P04 | V | an `actionview` release with `pin` `7.2.2` or `admittedLine` `7.2` | Rails 7.x is legacy |
| P05 | V | a release with no `source`, or a `source` not naming the exact pin | pin without its named source |
| P06 | V | a profile listing two `engineRelease` values, or a versions array | one profile holds exactly one release |
| P07 | V | a release `pin` outside its own `admittedLine` (`1.14.0` in line `1.13`) | pin diverged from its admitted line |
| P08 | V | a second erubi profile for an unannounced major (`erb.erubi-2`) while `nextMajor.announced` is false | next major admitted before announcement and ratification |
| P09 | V | a profile with no `tags`, no `trimModes` or no `closeRule` | profile does not record its tag set, trim modes and first-`%>` close rule |
| P10 | V | `closeRule` changed to a rule that skips `%>` inside Ruby strings | close rule diverges from both engines (first `%>`) |
| P11 | V | `<%==` listed as a tag of `erb.erb-6.0` | fabricated feature: the erb gem has no `<%==` tag |
| P12 | V | `%%>` listed as a close marker of `erb.erubi-1.13` | fabricated feature: Erubi does not recognise `%%>` |

## ERB0-AC2 — owned matrix (`ERB0-matrix`)

| Id | Product | Planted row | Expected failure |
| -- | ------- | ----------- | ---------------- |
| M01 | M | a cell with no `producer` | unowned cell |
| M02 | M | two cells with the same `operation`, `host` and `profile` and different producers | duplicate owner |
| M03 | M | a cell with an empty `acceptance` list, or an id not prefixed by its producer | cell without a receiving acceptance item |
| M04 | M | an operation present for one profile and missing for the other | profile coverage hole (ERBT compares profile for profile) |
| M05 | M | a cell whose operation is `render.template` | render cell: no runtime |
| M06 | M | a cell whose operation runs Ruby or Bundler (`ruby.evaluate`) | Ruby-execution cell |
| M07 | M | a host with a non-empty `tsgoOperations`, or a cell on a tsgo host | tsgo route for a carrier document |
| M08 | M | a cell whose host or producer targets `packages/typescript-plugin` | TS plugin cell |
| M09 | M | a cell for a Ruby semantic operation (`ruby.hover`) | fabricated feature: Ruby is opaque |
| M10 | M | an exclusion that also appears as an admitted cell | an operation is either a cell or an exclusion |
| M11 | M | an exclusion with no `reason` | untruthful exclusion |
| M12 | M | a cell whose evidence cites a TextMate grammar or syntax highlighting | a grammar never satisfies a product claim |
| M13 | M | `wireTag` set to a `FrameworkTag` value, or `facets` non-null | server-template carriers have no tag and no facets |
| M14 | M | a cell producer outside ERB1, ERBH1, ERB2, ERB3, ERB4S, ERB5F, ERB4, ERBA, HRF1-ERB, ERBP0, ERB9, ERBT | producer not in the ratified set |

## ERB0-AC3 — activation and associations (`ERB0-activation`)

| Id | Product | Planted row | Expected failure |
| -- | ------- | ----------- | ---------------- |
| A01 | A | a source activating `erb` because a `*.html.erb` file exists | activation from a file name |
| A02 | A | a source activating `erb` from a `<script src>` URL | activation from a URL |
| A03 | A | a source reading `Gemfile` requirements | activation from an unread Gemfile (declared, not resolved) |
| A04 | A | a source invoking `bundle` or `ruby` | Ruby or Bundler process |
| A05 | A | a `*.js.erb` association with claim `gated` or `configured` | non-HTML host claimed |
| A06 | A | a `*.text.erb` association that is claimed | non-HTML host claimed |
| A07 | A | a plain `*.erb` association with claim `gated` | plain `.erb` claimed without configuration |
| A08 | A | a state row where `on` without a profile gives `active` | `on` without a named profile |
| A09 | A | a state row where a lockfile-free `auto` gives `active` | activation without a lockfile or explicit `on` |
| A10 | A | selection order with `erb` before `actionview` | selection order is actionview, then erubi, then erb |
| A11 | A | a gem outside its admitted line falling back to a later gem | an unsupported version never falls back |

## ERB0-AC4 — coexistence declared (`ERB0-coexistence`)

| Id | Product | Planted row | Expected failure |
| -- | ------- | ----------- | ---------------- |
| C01 | C | the `coex.lint.herb` cell removed | competitor-provided capability without a COXD1 cell |
| C02 | C | the `coex.format.herb` cell removed | competitor-provided capability without a COXD1 cell |
| C03 | C | the `coex.html-features.ruby-lsp` cell removed | competitor-provided capability without a COXD1 cell |
| C04 | C | a cell naming a tool other than `ruby-lsp` or `herb` | undeclared competitor |
| C05 | C | a cell whose `operation` is not a matrix operation | cell on an unknown operation |
| C06 | C | a matrix operation in neither `cells` nor `noCompetitorKnown` | undeclared coexistence state |
| C07 | C | an operation in both `noCompetitorKnown` and `cells` | contradictory coexistence state |

## Positive cases

| Id | Expectation |
| -- | ----------- |
| OK1 | The four clean products pass. |
| OK2 | Every matrix operation has exactly one cell per profile and one producer. |
| OK3 | Every profile id the activation policy selects exists in the version lock. |
| OK4 | Every coexistence `operation` exists in the matrix. |
