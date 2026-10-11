# Liquid: delivery and exact-version contract

This is the stage-0 contract for the Liquid family. Every later Liquid node
(LIQ1 to LIQT, HRF1-LIQ) and the translation nodes LIQ11 and LIQ11T build on it.
It fixes four dialect profiles with exact pins, the vocabulary each dialect
admits, how FWA1 activates Liquid and picks the dialect, how `.liquid` files
are associated, and the operation × host × dialect matrix with one producer and
one acceptance item per cell.

It describes the repository at `docs(arch): record the parser decision,
ownership, reuse and lineage (#812)`, 2026-10-09. At that head the repository
has no Liquid code: no parser, no language row, no activation and no client
contribution. Like the [kernel decision tier](kernel/README.md), this contract
is docs-only. It changes no production route and adds no validator, test, CI
gate or schema enforcement.

## Machine-readable products

The reviewed contract data lives in `tests/framework-liquid/LIQ0/`:

| File | Holds |
| ---- | ----- |
| `manifest.json` | The lock's products, its four acceptance cases with their planted twins, and the validator obligation that LIQ1G owns |
| `cases.md` | The clean leg and every planted row of `LIQ0-AC1` to `LIQ0-AC4`, each with its expected failure reason |
| `products/liquid-version-lock.json` | The four profiles: release package, exact reference pin, admitted line, registry, integrity and licence; pinned data; engine dependencies; next lines and excluded lines |
| `products/liquid-vocabulary.json` | Per dialect, the tag, filter and object tables, each with its exact source artifact and path |
| `products/liquid-activation-policy.json` | The FWA1 `liquid` activation row, the sources that never activate, and the `.liquid` association with `NeedSelection` |
| `products/liquid-capability-matrix.json` | Cells `LQM01`–`LQM74`, qualification rows `LQQ1`–`LQQ7`, exclusions `LQE1`–`LQE7`, competitors, and COXD1 cells `LQX01`–`LQX17` (`LQX03`, `LQX05` unassigned) |

FWA1 reads the version lock and the activation row as data. LIQ4 reads the
vocabulary as its only object and filter source. FCH1's coverage join and LIQ9
read the matrix. REG0's lock runner finds the validator once LIQ1G adds it.

## Dialect profiles

Each profile is one FWA1 `mode` and one release. FWA1 admits a resolved
installed version inside the profile's admitted line as its own exact release.
Anything else is `unsupported-version` and is never coerced (VID0 `R04`, `R06`,
`R09`). The reference pin is the release whose official tables populate the
vocabulary.

| Profile (`mode`) | Release package | Reference pin | Admitted line | Also pinned |
| ---------------- | --------------- | ------------- | ------------- | ----------- |
| `shopify` | `liquid` (rubygems) | 5.14.0 | 5 | theme data: `@shopify/theme-check-docs-updater` 3.30.1 (catalogs and theme JSON schemas bundled in the tarball); `@shopify/liquid-html-parser` 2.10.2 (the `schema` raw tag) |
| `jekyll` | `jekyll` (rubygems) | 4.4.1 | 4.4 | engine `liquid` 4.0.4, required by Jekyll 4.4.1 as `liquid ~> 4.0` |
| `liquidjs` | `liquidjs` (npm) | 10.30.0 | 10 | none |
| `eleventy` | `@11ty/eleventy` (npm) | 3.1.6 | 3 | engine `liquidjs` 10.30.0, which must equal the `liquidjs` reference pin |

Points the pins settle:

- **Shopify has no installable engine version.** A theme vendors no engine,
  so a theme-root activation admits the `shopify` reference release with
  provenance `theme-root`. The release is the platform engine's open-source
  line plus the pinned theme data.
- **Jekyll runs Liquid 4.** Jekyll 4.4.1 depends on `liquid ~> 4.0`. The
  `jekyll` dialect therefore has no `render`, `echo`, `liquid`, `doc` or inline
  `#` tag, and its filter base is Liquid 4.0.4's. The engine belongs to the
  admitted Jekyll release; it is not an admitted `liquid` 4 line. That is why
  the legacy-major rule does not reject it (`cases.md`, LIQ0-AC1).
- **Next lines are not admitted.** LiquidJS 11 exists only as
  `11.0.0-alpha.1` under the npm `next` tag. Eleventy 4 exists only as
  `4.0.0-alpha.10` under `canary`. Neither is a profile. Either becomes a
  second profile only through a ratified amendment of this lock.

Integrity values are the registries' published digests: rubygems SHA-256 of
the `.gem`, npm `sha512` integrity of the tarball. They are dependency pins,
not historical proof.

## Vocabulary

`liquid-vocabulary.json` holds, per dialect, the tag, filter and object
tables. Each table names one source: package, registry, pin and the path inside
the published artifact, relative to the artifact root (the gem root, or the
npm tarball's `package/` directory). A name shared by two dialects appears in a table of
each, with that dialect's source. Nothing is inferred from a spelling or
borrowed from another dialect.

| Dialect | Tags | Filters | Objects |
| ------- | ---- | ------- | ------- |
| `shopify` | 21 engine (`Tags::STANDARD_TAGS` plus `liquid`), 9 platform (`data/tags.json`), `schema` (`RAW_TAGS`) | 60 engine (`StandardFilters`), 94 platform (`data/filters.json`), the `t` alias of `translate` | 34 global and all 142 catalog objects (`data/objects.json`) |
| `jekyll` | 16 engine (Liquid 4.0.4), 5 Jekyll (`highlight`, `include`, `include_relative`, `link`, `post_url`) | 48 engine, 33 Jekyll | 9 payload names (`UnifiedPayloadDrop`) |
| `liquidjs` | 21 (`new Liquid().tags`) | 88 (`new Liquid().filters`) | none: context comes from user code |
| `eleventy` | the 21 LiquidJS tags plus 2 default bundle shortcodes (`getBundle`, `getBundleFileUrl`, from `@11ty/eleventy-plugin-bundle` 3.0.7, which Eleventy adds unconditionally) | the 88 LiquidJS filters plus 10 Eleventy defaults | 5 data-cascade names (`collections`, `content`, `eleventy`, `page`, `pkg`) |

Block delimiters (`else`, `elsif`, `when`, `end<tag>`) are grammar, not tags.
Under a dialect, a tag outside its tables is an opaque node (LIQ1-AC3), and a
filter outside them is Unavailable with a reason, never `any` (LIQ4-AC2).
Populations that only executing code could reveal are `unknown`: Jekyll
`_plugins/*.rb` and plugin gems, LiquidJS `registerTag`/`registerFilter`
calls, and computed or plugin Eleventy registrations (LIQ5J).

## Activation

FWA1's `FrameworkActivation` record is the only activation source. The dialect
is the record's `mode`, and a mode is exactly one profile.

| Source | Mode | Evidence | Provenance |
| ------ | ---- | -------- | ---------- |
| AP-S0 | (all) | `frameworks.liquid = off` turns off the whole vertical | explicit |
| AP-S1 | `shopify` | one directory holds both `layout/theme.liquid` and `config/settings_schema.json` | theme-root |
| AP-S2 | `jekyll` | `jekyll` resolved in the package's `Gemfile.lock`, read as data | lockfile |
| AP-S3 | `eleventy` | `@11ty/eleventy` resolved in the package graph | manifest |
| AP-S4 | `liquidjs` | `liquidjs` resolved as a direct dependency of the package, with or without `@11ty/eleventy` | manifest |
| AP-S5 (rank 0) | named | `frameworks.liquid = { state: on, release: <exact admitted release> }` | explicit |
| AP-S6 | none | none of the above: `.liquid` files fall back and no Liquid parse runs | none |

The explicit `on` uses CFG0's shape. A release decodes to exactly one profile,
so naming the release names the profile. The switch (AP-S0, AP-S5) ranks
above the automatic sources AP-S1..AP-S4, which never outrank each other.
Only the LiquidJS that Eleventy resolves through its own dependency edge is
Eleventy's engine and not a second claim; a `liquidjs` the package declares
itself is an independent AP-S4 claim, because package co-presence proves
neither one instance nor one profile for every `.liquid` file.

AP-S4 narrows the charter's "`liquidjs` resolved in the package graph" to a
direct dependency: a `liquidjs` reached only through a dependency other than
Eleventy is that dependency's private engine and names no dialect for the
package's files, as a bare `liquid` gem names none. Such a package needs an
explicit `on`.

These never activate: a `.liquid` extension; a `<script src>` naming a Liquid
build; any directory evidence other than the two theme-root files together;
the `liquid` gem in `Gemfile.lock` without `jekyll`; a `liquidjs` resolved only
transitively through a dependency other than `@11ty/eleventy`; a dialect read from file
contents or a declared version range.

FWA1 can carry this row as written. Its record has a `mode` field, and its
2026-10-01 amendment adds the theme root and `Gemfile.lock` as static sources
read as data, so the charter's abort condition does not apply.

## The `.liquid` association

A `.liquid` file takes the mode of the one active claim whose scope contains
it. A file under two claims naming different profiles is `NeedSelection`:
different modes (a theme root inside a Jekyll site, a Jekyll `Gemfile.lock`
and an Eleventy package over one directory, an Eleventy package that also
declares `liquidjs`), or one mode at two exact releases (nested
`Gemfile.lock` files resolving `jekyll` 4.4.0 and 4.4.1). Each release is its
own profile identity. Under `NeedSelection` there is no Liquid parse in either
profile and no fact, and the explain output names both claims.
Only an explicit `on` naming one profile resolves it. Nesting depth,
registration order, load order and recency never decide (VID0 `R10`). Two
claims are one claim only when they name the same profile at the same exact
release; the nearer scope never chooses a release.

LIQP0 owns document selectors and CLI globs. LIQ1G owns the client language
contribution: language `liquid`, extension `.liquid`, grammar scope
`text.html.liquid`.

## Operation × host × dialect matrix

The matrix has 74 cells. Each cell has one producer and one receiving
acceptance item, and for each of the four dialects it is either admitted or
excluded with a reason.

- **Hosts.** `verter-lsp`: the Verter LSP serves `.liquid` documents itself.
  `session`: the versioned facts those operations read. `mcp`: DX1 operations
  through the generated router. `cli`: `verter lint` and `verter format`.
  `vscode-client`: client registration.
- **No tsgo route.** There is no TS projection and no
  `packages/typescript-plugin` work, so no cell needs a tsgo operation or has a
  tsgo limitation.
- **Capability keys.** These are LSP method names or Verter operation ids.
  Reconciling them to the COX0D capability vocabulary is UAP0's work.

| Producer | Cells | What they own |
| -------- | ----- | ------------- |
| LIQ1 | LQM01–LQM04 | lossless parse, recovery, dialect tag tables, raw bodies (parser: NewParser with owner-local dialect tables, PAR0 `CL17`, home `H07` `crates/verter_liquid_syntax`) |
| LIQ3, LIQP0 | LQM05–LQM06 | inactive is zero work; dialect selection with provenance |
| LIQ1S | LQM07–LQM11 | native syntax diagnostics, symbols, folding and selection, carrier semantic tokens, gating and incrementality |
| LIQ2 | LQM12–LQM16, LQM74 | templated-HTML composition through ERBH1; embedded regions through EMB0, including authored `<script>` and `<style>` |
| LIQ3 | LQM17–LQM20 | scopes, loops, partial resolution, incremental index |
| LIQ4 | LQM21–LQM24 | render contracts, catalog types, static data |
| LIQ5 | LQM25–LQM28 | Shopify settings, template references, schemas, dialect gating |
| LIQ5J | LQM29–LQM32 | Eleventy literal registrations, computed-is-unknown, never-executed, Jekyll links |
| LIQ6 | LQM33–LQM40 | completion, definition and references, HTML once, hover and signature help, linked editing, partial results, `liquid.snippets`, `liquid.settings` |
| LIQ6R | LQM41–LQM44 | rename: merchant data refusal, snippet move, scope exactness, step-down |
| LIQ7 | LQM45–LQM48 | style and accessibility facts through holes |
| LIQ8 | LQM49–LQM53 | lint pack, incomplete populations, markup through holes, Theme Check parity |
| LIQ8F | LQM54–LQM57 | formatting |
| LIQA | LQM58–LQM61 | assists |
| HRF1-LIQ | LQM62–LQM65 | HTML-response fragment facts |
| LIQP0 | LQM66–LQM68 | client manifest, CLI globs, capability truth when off |
| LIQ11, LIQ11T | LQM69–LQM73 | Shopify translation keys and `liquid.translations` |

Dialect exclusions follow from the pinned engines. Jekyll has no `render`, so
LQM17, LQM47 and LQM49 exclude it. Only Shopify's engine has LiquidDoc's
`doc` tag, so render contracts (LQM21) and extract-snippet (LQM58) are
Shopify-only. Theme constructs (schema, settings, sections, `templates/*.json`,
locales) are Shopify-only. Front matter and static data are Jekyll and
Eleventy only.

Qualification sits outside the cells. LIQ9-AC1 joins every admitted cell to a
passing fixture, per dialect. LIQT-AC4 promotes only at the full matrix.
LIQT-AC1 and LIQ11T-AC1 prove zero-work step-down. This lock is static proof
and claims no support (WDX0 claim laws). The LIQ1G TextMate grammar is never a
cell (LIQ1G-AC6).

## Dispositions

- **Snippets** are carrier render contracts from LiquidDoc `@param`, not
  ComponentInfo.
- **Facets.** `props`, `events`, `slots` and `expose` are absent.
- **Wire tag.** There is no `FrameworkTag`.
- **Exclusions.**
  - LQE1: no rendering, and no cell whose answer is render output.
  - LQE2: no `jekyll build`, Eleventy build or `shopify theme dev`.
  - LQE3: no TS plugin host.
  - LQE4: no tsgo projection, and nothing re-provides a TS answer.
  - LQE5: no ComponentInfo facets for snippets.
  - LQE6: no execution of Eleventy config, `_data/*.js` or Jekyll plugins.
  - LQE7: no LiquidJS 11 or Eleventy 4 profile.

## Coexistence (COXD1 cells)

Three competitors are declared:

- **Shopify Liquid**: VS Code extension `Shopify.theme-check-vscode`. It
  provides completion, hover, definition, rename, formatting, linked editing,
  diagnostics and code actions. Its language server advertises no references
  or signature help, so neither gets a coexistence cell.
- **Theme Check**: `@shopify/theme-check-node`, also run by that extension and
  by `shopify theme check`. It provides diagnostics, fixes and CLI lint.
- **i18n Ally**: `Lokalise.i18n-ally`, on the LIQ11 translation cells only.

Each capability a competitor provides that a cell also has gets one COXD1
cell, `LQX01`–`LQX17` (`LQX03` and `LQX05` unassigned), listing exactly the
cells it covers. A competitor with a `scope` (i18n Ally) is matched only
against the cells inside that scope. COXD2 decides
ownership per capability at runtime; Unknown means Verter owns.

## Predecessor contracts

- **WDX0.** Claim and routing law. This lock is static proof. Support is
  claimed only from runtime observation at the family's terminal.
- **PAR0.** The in-house Liquid parser: row `CL17` is NewParser with dialect
  tables, home `H07`. The four profiles are the `DK2` dialect identities that
  enter the syntax profile. PAR0's `PAR-F07` already records that LIQ1's
  charter places grammar work in `verter_language` while the home is `H07`.
- **VID0.** One release per manifest, separate identities per major, no
  implicit default major, and activation from the resolved installed version.

## Moved obligations

Every "validator rejects" and "planted … fails" proof of LIQ0-AC1 to LIQ0-AC4
moves unchanged to LIQ1G as LIQ1G-ACV. LIQ1G adds
`tests/framework-liquid/LIQ0/contract.ts` and `liquid-lock.spec.ts`. They
implement every row of `cases.md`, and REG0's runner discovers them. Until
then the products are reviewed data, not CI-checked.

## Observed gaps

- **Hover, signature help and linked editing.** LIQ6's outcome names them,
  but no LIQ6 acceptance item does. Their cells (LQM36, LQM37) take LIQ9-AC1 as
  receiving acceptance, with an `acceptanceNote`. A LIQ6 acceptance item for
  each would make the producer's own test discriminate them.
- **Authored `<script>` and `<style>`.** LIQ2's outcome attaches them through
  EMB0, but LIQ2-AC3 names only `{% schema %}`, `{% stylesheet %}` and front
  matter. Their cell (LQM74) takes LIQ9-AC1 with an `acceptanceNote`. A LIQ2
  acceptance item for them would make the producer's own test discriminate
  them.
