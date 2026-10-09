# Marko delivery and exact-version contract

This is the contract that every later Marko node builds against: the MRK
train, the `lint.marko` packs (LMK1–LMK3) and the Marko semantic-fact
contributions (EFF3-MARKO and the forwarding and render contributions). It
fixes the admitted release, the activation rule, the parser decision for each
`.marko` sublanguage, the operation × host × profile matrix and the facet
mapping.

It describes the repository at `docs(arch): record the parser decision,
ownership, reuse and lineage (#812)`, 2026-10-09. It is docs-only, following
the rule in [kernel/README.md](kernel/README.md): it adds no production code,
CI gate, validator or test. It builds on the
[web-product ownership constitution](../../tests/web-product/WDX0/manifest.json)
(WDX0), the [parser decision](kernel/parser-ownership.md) (PAR0) and the
[identity and exact-release law](kernel/identities.md) (VID0), and re-owns
nothing they assign.

## Machine-readable products

The reviewed contract data lives in `tests/framework-marko/MRK0/`:

| File | Holds |
| ---- | ----- |
| `manifest.json` | The products, their readers, the acceptance cases and the validator handover to MRK1G |
| `products/marko-version-lock.json` | The admitted release, the excluded releases `XR01`–`XR04`, the test-only oracles and the wire tag |
| `products/marko-activation-policy.json` | The FWA1 activation row, the zero-work rule and the host policy |
| `products/marko-capability-matrix.json` | Profiles, hosts, parser decisions, cells `C01`–`C56`, exclusions `X01`–`X07` and facets |
| `cases.md` | Every planted row and the reason it must fail, per acceptance id |

REG0, FWA1 and FCH1 read the products as data. Nothing checks them until
MRK1G lands `contract.ts` and `marko-lock.spec.ts` (`MRK1G-ACV`), which REG0's
`scripts/run-framework-locks.mjs` discovers (`REG0-AC6`).

## Baseline

At the described head:

- No `.marko` `LanguageRow` exists in `crates/verter_language/src/registry.rs`.
- `CarrierGrammarConfig` (`crates/verter_language/src/carrier_grammar.rs`)
  has only `Vue` and `Svelte`.
- The wire `FrameworkTag` enum (`crates/verter_protocol/proto/verter/v1/typeinfo.proto`)
  holds `NONE 0`, `VUE 1`, `SVELTE 2`, `REACT 3`, `SOLID 4` and
  `OPEN_CANONICAL 5`. `tag_disposition`
  (`crates/verter_session/src/framework/registry.rs`) returns `None` for
  `NONE` and `OPEN_CANONICAL`.
- `FileLanguage::Framework { adapter_id, language_id }`
  (`crates/verter_language/src/language.rs`) is an open set, so Marko needs no
  new language kind.
- No production source, generated mirror or client contribution names Marko.
- There is no Marko architecture-proof node, so this contract carries the
  displaced-route inventory itself (below).

## Decisions

### 1. Release

The admitted release is exactly **marko 6.4.3** (Tags API), in its own
release manifest (`marko@6.4.3`, profile `marko-6-tags@6.4.3`). The oracles
are exactly `@marko/compiler` 5.42.10, `@marko/language-tools` 2.7.0 and
`@marko/type-check` 3.2.1, the set the LMK1 profile locks.

6.4.3 is the newest published 6.4.x whose declared `@marko/compiler` range
(`^5.42.10`) admits the locked compiler oracle. 6.4.4 and 6.4.5 declare
`^5.42.11`, which excludes it, so admitting them would split the product
release from its oracle set (`XR01`). Moving to a later patch is one ratified
change to this lock and the LMK1 profile together.

The **next 6.x is excluded** (`XR02`). No later 6.x minor is published, so
there is no exact version to pin, and the npm `next` tag points at 6.1.8,
older than the admitted release. A later 6.x becomes a second admitted profile
only through a new exact release manifest ratified here.

**Marko 5 Class API is excluded** (`XR03`): `class {}` component blocks,
`this.state`, `this.input` on a component instance, `<include>`, and sibling
`component.js` / `component-browser.js` files. An installed Marko 5 is
`unsupported-version`. A Class API construct inside an admitted package is a
typed `SyntaxReject` (`MRK1-AC4`), never a silent Tags-API parse. LMK3
reports mixing. Pre-releases, canaries and nightlies never decode (`XR04`).

### 2. Activation

FWA1's `FrameworkActivation` record is the only activation source. Marko is
active for a package only when the PM3 snapshot's resolved installed version
of `marko` equals an admitted release, or when `frameworks.marko = on` names
an admitted release (provenance `explicit`). `off` disables the vertical;
`on` naming an unadmitted release is `unsupported-version`.

When the record is `inactive`, `unsupported-version` or `off`, the whole
Marko vertical does zero work for the package: no parse, region attachment,
carrier feature, binding, projection, fact, index, tsgo companion, lint-host
or format work (`MRK1M-AC5`, `MRK1S-AC5`, `MRK8-AC1`). Never activation
sources: the `.marko` extension, the declared range, a string-prefix version
read, another `@marko/*` package alone, or a `marko.json` file. LK6 keeps
only the `lint.marko` pack switches. FWA1's admitted result sets the Marko
`ProjectCapabilitySnapshot` bit, which gates the `.marko` `LanguageRow::gated`
row.

FWA1 expresses a per-package, version-admitted record, so this contract's
abort condition does not apply.

### 3. Parser lineage

| Sublanguage | PAR0 row | Decision | Home |
| ----------- | -------- | -------- | ---- |
| Markup, HTML mode | `CL13` | NewParser (`DK4`) | `H04` `crates/verter_marko_syntax` (MRK1) |
| Markup, concise mode | `CL13` | the same frontend, mode held per line | `H04` |
| JS/TS in attribute values and arguments, placeholders, tag parameters, tag variables, statements, `<script>`, method-shorthand attributes | `CL01` | Reuse (OXC) | `G01` |
| `style {}` and `<style>` | `CL04` | Reuse | `verter_css_syntax` |
| `marko.json` taglib configuration | `CL29` | Reuse, read as data | the JSON owner |

**The Marko frontend neither reuses nor forks the HWC1 HTML tokenizer
(`H08`).** Marko markup is not WHATWG HTML with additions:

- concise mode is indentation-structured and has no HTML tokenizer correlate;
- unquoted attribute values, attribute arguments, tag parameters `|…|`, tag
  variables `/x` and `${}` placeholders are JavaScript expressions whose
  extent the HTML attribute-value states cannot find;
- statements at line start and `--` text blocks change what a line means.

A fork would remove most of the neutral parser's states and re-add a
different lexical model, which is a new parser in all but name. PD06 forbids
a Marko branch in the neutral parser, and at this head `H08` does not exist
yet, so no candidate exists to measure against. Shared-tokenizer extraction
stays reserved under PD06. `@marko/compiler`, and the parser it depends on,
are oracles only.

### 4. Host route per operation (ruling 1)

- **TS regions** — placeholders, attribute expressions, tag parameters, tag
  variables, statements and `<script>` — get hover, completion, definition,
  references, rename and diagnostics from tsgo through the Verter LSP's
  TypeProvider tsgo routes, over the MRK6 companion.
  `TsgoCompositeProvider` (`crates/verter_lsp/src/tsgo/composite.rs`) tries
  shared tsgo first (`TsgoSharedProvider`, the relay-shim attach), then
  managed tsgo (`TsgoOwnedBackend` over `crates/verter_tsgo_api`). Companion
  sync is `crates/verter_lsp/src/tsgo/carrier_sync.rs`, and answers map back
  through `ProviderPositionMapper` with generated-only spans suppressed. A
  TS-region answer Verter computes itself is a duplicate.
- **Carrier-only operations** are Verter LSP enhancements: native HTML tag
  and attribute completion, attribute-tag and custom-tag name completion,
  concise and HTML structure symbols, folding and selection, carrier syntax
  diagnostics, carrier semantic tokens and HTML-mode auto-close. Each answers
  only where LSPX11's `owns(capability, position)` resolves to Verter.
- **tsserver:** nothing is forwarded. `@verter/typescript-plugin` keeps only
  its carrier-generic carrier-store compatibility (`MRK6-AC6`).

Every TS-region row is a manifest row over the existing tsgo routes. No new
client transport is needed, so the rescope trigger does not fire.

### 5. Facets (ruling 3)

| Facet | Source | Provenance | `FrameworkSurfaceKind` | In `supported_surfaces` | Receiving acceptance |
| ----- | ------ | ---------- | ---------------------- | ------------------------ | -------------------- |
| props | `Input` | native | `PROPS` | yes | `MRK3-AC1` |
| events | function-typed `on*` inputs | `derived(callback-input)` | `EMITS` | yes | `MRK3-AC3` |
| slots | `content` plus attribute tags `<@x>` | `derived(attribute-tag)` | `SLOTS` | yes | `MRK3-AC3` |
| expose | the `<return>` value (Approximate inside a conditional) | `derived(return-tag)` | `EXPOSE` | yes | `MRK3-AC3` |
| options, model | — | UNSUPPORTED | `OPTIONS`, `MODEL` | no | `MRK3-AC4` |

Facets map onto the existing TIF1 wire enum `FrameworkSurfaceKind` through
`FrameworkAdapterDescriptor.supported_surfaces`; the Marko events facet is
the existing `EMITS` kind, and no Marko-specific kind is added. A kind
absent from `supported_surfaces` — options, model and any kind with no Marko
facet — is filled structurally as `UNSUPPORTED` by the executor, never as a
supported-empty kind, and every record of a mapped kind carries its
provenance. MRK1 registers the descriptor with an empty `supported_surfaces`
(every kind `UNSUPPORTED`, `MRK1-AC6`); MRK3 then sets it to exactly
`PROPS`, `EMITS`, `SLOTS`, `EXPOSE` without another wire change. The
matrix records this as `facetWireMapping`.

### 6. Wire tag

`FRAMEWORK_TAG_MARKO = 12` is ratified from the framework wire-tag table. It
lands in MRK1 with the adapter descriptor (`MRK1-AC6`). `OPEN_CANONICAL` (5)
is a structural non-tag and is rejected, as are any baseline value (0–5),
any class-A value (6–9) and any value allocated to another family.

### 7. Build exclusion (ruling 4)

Official `@marko/compiler` and bundler output, streaming output and client
entries are not consumed, and no DBG, TST or WPF map is promised (`X01`–`X03`).
Rendering, hydration and executing components are runtime work (`X04`). MRK6
is the TS-host projection only.

## Matrix

`marko-capability-matrix.json` holds 56 cells over the one admitted profile.
Each cell names one producer node and one ratified receiving acceptance id
of that producer requiring proof of the operation. Null receivers and
pending-receiver notes are invalid. A ratified receiver records an obligation;
it does not claim that the feature is implemented or qualified.
Every cell records `tsgoOperations` and `tsgoLimitation` explicitly; an
empty list and `null` mean none. On a kernel cell (`C20`–`C22`) the tsgo
operations are checker queries the producer consumes as type-authority
input; tsgo answers only tsgo-hosted cells.

| Producer | Cells | Hosts |
| -------- | ----- | ----- |
| MRK1 | `C01`–`C05` | verter-kernel |
| MRK1M | `C06`–`C10` | verter-kernel |
| MRK1S | `C11`–`C15` | verter-lsp |
| MRK2 | `C16`–`C19` | verter-kernel |
| MRK3 | `C20`–`C23` | verter-kernel |
| MRK4 | `C24`–`C27` | verter-kernel |
| MRK5 | `C28`–`C32`, `C56` | tsgo, verter-lsp |
| MRK5R | `C33`–`C37` | tsgo, verter-kernel, verter-lsp |
| MRK6 | `C38`–`C43` | tsgo, verter-kernel, ts-plugin |
| MRK7 | `C44`–`C47` | verter-kernel |
| MRK8 | `C48`–`C50` | lint-host |
| MRK8F | `C51`–`C54` | format-host |
| MRK1G | `C55` | editor-client, no support claim |

The excluded Marko 5 and next-6.x profiles are exclusions `X05` and `X06`,
and tsserver forwarding is `X07`. Lint rule populations belong to the
`lint.marko` packs; MRK8 is the host only (`MRK8-AC4`). MRK9 joins every cell
to a passing fixture (`MRK9-AC1`), and MRK10 promotes only a full matrix
(`MRK10-AC1`).

## Displaced routes

| Id | Route at this head | Disposition | Owner |
| -- | ------------------ | ----------- | ----- |
| `M-D01` | A `.marko` file has no language row and falls through to unclassified handling | replace with a gated `LanguageRow` behind the FWA1 capability bit | MRK1 (`MRK1-AC6`), MRK1M (`MRK1M-AC5`) |
| `M-D02` | `CarrierGrammarConfig` is a closed `Vue`/`Svelte` enum | add the Marko grammar configuration through REG0's family module | MRK1 (`MRK1-AC1`) |
| `M-D03` | `FrameworkTag` and `tag_disposition` have no Marko value or arm | add `MARKO = 12`, `Registered` with every surface kind `UNSUPPORTED` until MRK3 | MRK1 (`MRK1-AC6`), MRK3 (`MRK3-AC4`) |
| `M-D04` | Framework release admission by string prefix (VID0 `V-D05`) is the existing version-admission route | never copied for Marko; activation reads the FWA1 record | FWA1 (`FWA1-AC1`), MRK1M (`MRK1M-AC5`) |
| `M-D05` | The client manifest and virtual-file-naming mirrors have no Marko row | render the rows from the descriptor | MRK6 (`MRK6-AC4`) |
| `M-D06` | The VS Code extension contributes no `.marko` language or grammar | contribute through REG0's generated fragment | MRK1G (`MRK1G-AC4`) |
| `M-D07` | The TS plugin's carrier routing is carrier-generic | keep it generic; no Marko-specific code | MRK6 (`MRK6-AC6`) |

MRK10 closes each row with its owner's acceptance (`MRK10-AC4`).

## Findings recorded for the receiving owners

- **F-MRK0-01 (MRK5).** Cells `C32` (auto-close) and `C56` (custom-tag name
  completion) name the ratified **MRK5-AC6 — carrier mode and discovery
  boundaries**. Through the Marko carrier feature provider, auto-close fires
  in HTML markup and is absent in concise mode and TS regions. Custom-tag
  name completion lists the canonical tags MRK2 discovers for the file's
  scope, respects nearest `tags/` scope precedence, and excludes names
  visible only in another package's `tags/` scope. The proof is
  `cargo nextest run -p verter_lsp -E 'test(/marko_carrier_features/)'`, with
  positive HTML/visible-tag controls and negative concise/TS/sibling-package
  controls that fail on planted boundary violations. MRK2-AC1 owns the
  discovery input; MRK5 consumes it without a second discovery path.
  MRK5 delivers this implementation and proof after MRK0. `MRK5-AC3` still
  covers native/attribute-tag completion only. Ratifying `MRK5-AC6` does not
  supply an MRK9 pass: full-matrix promotion (`MRK10-AC1`) still needs a pass
  or a ratified exclusion for every cell.
- **F-MRK0-02 (LMK1).** LMK1 states its marko line as "6.4.x latest stable".
  The registry's latest 6.4.x (6.4.5) excludes the shared compiler oracle, so
  that line resolves to this lock's exact 6.4.3.
- **F-MRK0-03 (MRK1).** `MRK1-AC1` refers to "the MRK0 grammar corpus". This
  contract fixes the constructs the corpus must cover — both modes, attribute
  tags, tag variables, tag parameters, dynamic tags, placeholders, statements,
  `style {}`, `--` text blocks and the Class API reject inputs — and MRK1 owns
  the corpus files.
- **F-MRK0-04 (CI path selection).** `tests/framework-marko/**` is listed as
  CI-inert in `scripts/ci-impact.mjs`, because nothing in `ci.yml` reads it
  until MRK1G lands the lock spec. MRK1G moves it to the lane that runs the
  spec.

## Acceptance evidence

Each acceptance is met here by the reviewed products and `cases.md`. Its
executable "planted … fails" proof moves unchanged to MRK1G (`MRK1G-ACV`).

| Acceptance | Met by |
| ---------- | ------ |
| `MRK0-AC1` pinned release | L `admittedReleases`, `excludedReleases`, `oracles`; cases `floating-range` … `oracle-floating` |
| `MRK0-AC2` owned matrix | M `cells`, `exclusions`, `cellLaw`; cases `unowned-cell` … `unqualified-cell-promoted` |
| `MRK0-AC3` activation and host policy | A `activationRow`, `zeroWorkWhenInactive`, `hostPolicy`, `parserAuthority`; cases `extension-only-activation` … `lk6-activation` |
| `MRK0-AC4` oracle is not support | M `cellLaw`, cell `C55` `supportClaim: none`, L oracle roles; cases `language-tools-evidence` … `syntax-highlighting-evidence` |
| `MRK0-AC5` wire tag ratified | L `wireTag`; cases `open-canonical-tag` … `missing-tag` |
