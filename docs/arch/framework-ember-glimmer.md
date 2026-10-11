# Ember and Glimmer delivery and exact-version contract

This decision fixes what the Ember/Glimmer vertical supports, how it is
activated, which parser owns each sublanguage, which host answers each editor
operation, and how components map onto the shared component facets. Every
later Ember/Glimmer node, the `lint.ember` packs and the forwarding,
rendering and effect consumers build against it.

It describes the repository at `docs(arch): define the compiler request,
policy and stage identity (#819)`, 2026-10-09. It follows the docs-only rule
in [the kernel README](kernel/README.md): it changes no production route and
adds no check. It builds on the
[identities](kernel/identities.md),
[configuration](kernel/configuration.md),
[demand](kernel/demand-activation.md) and
[parser](kernel/parser-ownership.md) decisions and does not re-own anything
they assign. Plan-node identifiers in this page are ownership bindings.

## Machine-readable products

The reviewed contract data lives in `tests/framework-ember-glimmer/GLM0/`:

| File | Holds |
| ---- | ----- |
| `manifest.json` | products, mandatory cases and the validator owner |
| `products/glimmer-version-lock.json` | admitted and excluded releases, unsupported modes, companions, oracles, wire tag |
| `products/glimmer-capability-matrix.json` | profiles, hosts, parser decisions, the operation × host × profile matrix, displaced routes, exclusions |
| `products/glimmer-activation-policy.json` | the FWA1 activation row, gated language rows, forbidden activation sources |
| `cases.md` | every planted row the lock validator must reject, with its failure reason |

The products are data. FWA1 reads the release table from the version lock.
The executable validator (`contract.ts`, `glimmer-lock.spec.ts`) belongs to
GLM1G (`GLM1G-ACV`); until it lands, nothing checks these files in CI.

## Baseline

- There is no Glimmer `LanguageRow`, no `CarrierGrammarConfig` variant
  (`crates/verter_language/src/carrier_grammar.rs` has Vue and Svelte only)
  and no `FrameworkTag` value (`typeinfo.proto` has `NONE`, `VUE`, `SVELTE`,
  `REACT`, `SOLID`, `OPEN_CANONICAL`).
- `.gjs` and `.gts` are unknown extensions and route as TypeScript scripts
  through `FileLanguage::script_ts()`. OXC cannot parse the `<template>` tag,
  so such a file fails to parse as a whole. This is the displaced route the
  vertical replaces in active packages; GLM10 (`GLM10-AC4`) closes it.
- `FileLanguage::FrameworkTemplate` exists for owned external templates and is
  the classification co-located `.hbs` files take (GLM1M).

## Decisions

### 1. Release and modes

| Item | Decision |
| ---- | -------- |
| Admitted release | `ember-source` **6.12.0**, the last published 6.x and the registry's `lts` on 2026-10-09 |
| Beta channel | **excluded**. The beta channel now ships `7.4.0-beta.1`, a 7.x prerelease; admitting it would admit an unratified major |
| 7.x stable | **excluded** by the ratified decision to admit 6.12.0 only (registry `latest` is `7.3.0`). 7.x, stable and beta, is reported unsupported with this reason; admitting it needs an explicit re-ratification and a re-pin of the lock |
| Other 6.x minors | excluded: one release per manifest |
| Alpha, canary, legacy (`< 6`) | excluded |
| Profiles | `strict-gjs`, `strict-gts` (strict-mode template tags) and `colocated-hbs` (loose-mode co-located templates with Glimmer components, including template-only) |

Unsupported modes, each reported with a reason and never a profile:
classic components (`@ember/component`), classic classes, `{{action}}`,
`{{mut}}`, implicit-this fallback and pre-Octane resolution.

Companion type authority: `@glimmer/component` 2.1.1. Test-only oracles:
`@glint/ember-tsc` 1.11.6, `@glint/template` 1.9.0 (the exact version
`@glint/ember-tsc` depends on), `@glint/tsserver-plugin` 2.7.9, `content-tag`
4.2.1, `@glimmer/syntax` 0.95.0, `ember-template-lint` 7.9.3 and
`eslint-plugin-ember` 13.6.0 (the line LEM1 locks). Prettier's glimmer
printer is not admitted as an oracle; GLM8F uses checked-in expected outputs.

### 2. Activation

Glimmer is active only where FWA1's `FrameworkActivation` record for the
owning package resolves `ember-source` to an admitted version. The states are:

- `active`: the full vertical runs.
- `inactive`, `unsupported-version`, `off` and `unproven`: zero work. That
  means no parse, attachment, fact, companion, lint, format or LSP carrier
  work.

An active record sets the family's `ProjectCapabilitySnapshot` bit, which gates
the `.gjs`, `.gts` and `.hbs` `LanguageRow::gated` rows. In an inactive
package, `.gjs`/`.gts` keep today's script route (`GLM1M-AC5`).

None of these activates the vertical: a file extension, `ember-cli-build.js`
or another file's presence, directory names, a declared range without a
resolved install, `@glimmer/component` or Glint without `ember-source`,
client-side colouring, or LK6. LK6 consumes FWA1 to select `lint.ember`
packs; it is not an activation source.

**Abort check:** not triggered. FWA1 publishes one record per package and
file per PM snapshot and admits by resolved installed version, and DEM0
`DR01` makes that record the only activation input.

### 3. Parser lineage (PAR0 `CL14`)

| Sublanguage | Decision | Home | Owner |
| ----------- | -------- | ---- | ----- |
| Glimmer template (`.hbs`, `<template>` content) | `NewParser` | `H05` `crates/verter_glimmer_syntax` | GLM1 (`GLM1-AC1`, `GLM1-AC2`) |
| `<template>` range discovery in `.gjs/.gts` | `NewParser` (same frontend) | `H05` | GLM1M (`GLM1M-AC1`, `GLM1M-AC2`) |
| `.gjs/.gts` host script | `Reuse` OXC (`CL01`) once ranges are attached | OXC | GLM1M (`GLM1M-AC1`) |
| CSS | not applicable: core Glimmer has no style blocks | — | — |

`NewParser` is chosen over `ForkAndSpecialize` of the HWC1 tokenizer because
mustaches, modifiers, sub-expressions, block params and whitespace control
occur inside element and attribute positions, so a fork would need Glimmer
tokenization in every HTML state. PD06 forbids a shared HTML-family parser.
`@glimmer/syntax` and `content-tag` are oracles only.

### 4. Host route per operation (ruling 1)

| Host | Serves |
| ---- | ------ |
| `tsgo` | Host-script and template-expression features: diagnostics, hover, completion, definition, references, TS rename and the import surface. They run over the GLM6 companion through `TsgoCompositeProvider`: shared tsgo (`TsgoSharedProvider`) first, then managed tsgo (`TsgoOwnedBackend`). Answers map back through `ProviderPositionMapper`, with generated-only spans suppressed. Co-located `.hbs` files are Verter LSP documents served the same way. |
| `lsp-enhancement` | Carrier-only operations: syntax diagnostics, document symbols, folding and selection, carrier semantic tokens, element and attribute completion, named-block, block and modifier-name completion where the projection cannot carry it, auto-close, loose-mode resolver-name navigation and carrier rename. They are answered only where LSPX11 resolves ownership to Verter. |
| `tsserver-carrier-store` | The existing carrier-generic carrier-store path of the TypeScript plugin, compatibility only (`GLM6-AC7`). |

Glint's language server and tsserver are never the host. No new client
transport is needed: every row uses the existing tsgo routes or a Verter LSP
carrier feature. The full per-profile cell table, with each cell's producer
and receiving acceptance, is the capability matrix product.

### 5. Facets (ruling 3)

| Facet | Source | Provenance | Owner |
| ----- | ------ | ---------- | ----- |
| props | `Signature['Args']` | native | GLM3 (`GLM3-AC1`) |
| slots | `Signature['Blocks']` | native | GLM3 (`GLM3-AC1`) |
| events | function-typed `Args` members | `derived(callback-arg)` | GLM3 (`GLM3-AC3`) |
| expose | — | `UNSUPPORTED`, never empty | GLM3 (`GLM3-AC4`) |
| root fact | `Signature['Element']` | native | GLM7 (`GLM7-AC1`) |

### 6. Wire tag

`FRAMEWORK_TAG_EMBER_GLIMMER = 13`, from the framework wire-tag allocation
table. It lands with the adapter descriptor in GLM1 (`GLM1-AC6`) and is
pre-allocated by REG0. It does not collide with the live tags (1–4), the
class-A range (6–9) or another family's planned value. `OPEN_CANONICAL` (5) is
a structural non-tag: `tag_disposition` returns `None` for it, and it never
identifies an adapter.

### 7. Build exclusion (ruling 4)

Embroider and ember-cli build output and their source maps are not consumed,
and no DBG, TST or WPF map is promised. GLM6 is the TS-host projection. No
cell renders, hydrates or executes Ember code or configuration.

## Displaced routes

| Route | Closed by |
| ----- | --------- |
| `.gjs/.gts` unknown-extension fallback to `FileLanguage::script_ts()` in active packages | GLM10 (`GLM10-AC4`) |
| Glimmer unregistered: no language row, carrier grammar variant or wire tag | GLM1 (`GLM1-AC6`) |
| The former build-output bridge claim; no such route exists in this tree | GLM6 (`GLM6-AC1`) |

## Acceptance at this stage

Each `GLM0-AC` is met by the reviewed products and the planted-row tables in
`cases.md`. The executable "planted … fails" proofs and the
`node --test tests/framework-ember-glimmer/GLM0/glimmer-lock.spec.ts` command
belong to GLM1G (`GLM1G-ACV`).

| ID | Met here by |
| -- | ----------- |
| `GLM0-AC1` pinned release | `glimmer-version-lock.json`; `cases.md` AC1 table |
| `GLM0-AC2` owned matrix | `glimmer-capability-matrix.json` (33 rows × 3 profiles, each cell owned or excluded); `cases.md` AC2 table |
| `GLM0-AC3` modes and activation | `unsupportedModes`, `glimmer-activation-policy.json`; `cases.md` AC3 table |
| `GLM0-AC4` proof is not support | the matrix `productEvidenceRule` and `oraclesOnly`; `cases.md` AC4 table |
| `GLM0-AC5` wire tag ratified | `wireTag` in the version lock; `cases.md` AC5 table |
