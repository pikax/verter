# Astro delivery and exact-version contract

This page is the readable form of the Astro family lock. It fixes the exact
Astro release, how Astro is switched on, which parser owns each part of a
`.astro` file, which host answers each editor operation, and how Astro
components map onto the framework-surface facets. Every later Astro node, the
Astro lint packs and the Astro semantic-fact contributions build against it.

It describes the repository at `docs(arch): record the parser decision,
ownership, reuse and lineage (#812)`, 2026-10-09, and the npm registry as
observed on 2026-10-09. It is contract text and data only: it changes no
production code and adds no check.

## Machine-readable products

The reviewed data lives in `tests/framework-astro/AST0/`:

| File | Holds |
| ---- | ----- |
| `manifest.json` | The product list, the five acceptance cases with their planted twins, the grammar corpus index and the upstream feature sources |
| `products/astro-version-lock.json` | The admitted release, the excluded next channel and lines, the test-only oracles, the wire tag, diverged pins and the build and runtime exclusions |
| `products/astro-capability-matrix.json` | Hosts in ruling order, the parser decision per sublanguage, the facet mapping and every operation × host × profile cell |
| `products/astro-activation-policy.json` | The FWA1 activation row, the `frameworks.astro` switch, the zero-work rule and the forbidden activation routes |
| `cases.md` | Every planted row with its expected rejection reason |
| `corpus/` | The Astro grammar corpus: valid files and recovery files |

The executable validator is not here. AST1G adds `contract.ts` and
`astro-lock.spec.ts` (`AST1G-ACV`), and REG0's `scripts/run-framework-locks.mjs`
discovers the spec. Until then FWA1, REG0 and FCH1 read the products as data.

## Baseline at the described head

- `ScriptRegionKind::Frontmatter` exists in
  `crates/verter_language/src/parse_artifact.rs`.
- `TopLevelOwnerKind::Frontmatter` exists. The enum is declared in
  `crates/verter_type_expr/src/facts.rs`; `verter_semantic`'s
  `root_binding_index.rs` consumes it.
- There is no `.astro` `LanguageRow`, no `CarrierGrammarConfig::Astro` (the
  enum holds `Vue` and `Svelte`) and no Astro `FrameworkTag` value (the proto
  stops at `FRAMEWORK_TAG_OPEN_CANONICAL = 5`).
- `.astro` literals are guarded by `single_language_classifier`
  (`crates/verter_session/tests/cases/g_misc0/single_language_classifier.rs`).
- The WDX1 fixture `tests/web-product/WDX1/fixtures/mixed-framework/case.json`
  pins `astro@5.0.0`. That pin is diverged; the lock records it and AST9
  re-pins the scenario.

## Release

| Item | Value |
| ---- | ----- |
| Admitted release | `astro` 7.3.8 (stable, npm `latest`, published 2026-10-08, MIT, `withastro/astro` `packages/astro`) |
| Profile | `astro-7.3.8`, the only admitted profile |
| Next channel | `astro` 7.4.0-beta.1, **excluded** |
| Not profiles | Astro 5, Astro 6, canary, `next--*`, `experimental--*`, alpha |

Admission follows the exact-release law (VID0 `R04`–`R08`). FWA1 compares the
package's resolved installed `astro` version for equality with 7.3.8; a
declared range is never read. Any other resolved version, including another
7.3 patch, is `unsupported-version` with its reason. Moving to a later release
is an amendment of this lock.

The next channel is excluded because 7.4.0-beta.1 is a prerelease minor of
the admitted major, not a next major, and framework-common admits only the
current stable major plus an announced next major. No Astro 8 prerelease is
published. When 7.4.0 ships stable, the lock is amended to it; an announced
Astro 8 prerelease may become a second profile only by a ratified amendment.

### Tooling oracles

Test-only, each a single npm devDependency of the conformance harness, never
a product parser, resolver, type owner or projection source. They are the
same set LAS1 locks. AST9 pins their transitive and platform package closure
hermetically; these top-level pins do not claim that an installation ran.

| Package | Version | Role |
| ------- | ------- | ---- |
| `@astrojs/compiler-rs` | 0.5.1 | grammar oracle |
| `@astrojs/astro2tsx` | 0.1.2 | projection oracle |
| `@astrojs/language-server` | 2.17.1 | editor-behaviour oracle |
| `@astrojs/check` | 0.9.10 | diagnostics oracle (`AST9-AC2`, `LAS2-AC1`) |

The [Astro 7.3.8 manifest](https://registry.npmjs.org/astro/7.3.8) declares
`@astrojs/compiler-rs` at `^0.5.1`, and the
[language-server 2.17.1 manifest](https://registry.npmjs.org/@astrojs/language-server/2.17.1)
declares `@astrojs/astro2tsx` at `^0.1.0`. The lock selects exact versions
from those ranges and records the published package integrity, licence and
source separately from the ranges. `@astrojs/compiler` 4.0.0 is excluded
from the oracle set because it is not the admitted release's parser.

Package provenance does not prove grammar, diagnostic or projection
compatibility. AST1 proves the dedicated frontend's grammar and recovery;
AST9 compares the oracles structurally in its hermetic harness. The AST6
projection is Verter-generated: upstream TSX, including `astro2tsx` output,
is comparison data only. No build or dev output is consumed.

## Activation

FWA1's `FrameworkActivation` record is the only activation source. Its Astro
row lives in REG0's `crates/verter_session/src/framework/families/astro.rs`:

- dependency name `astro`, no aliases, no per-file pragma, no non-npm route;
- admitted release `astro-7.3.8`, read from the version lock;
- `frameworks.astro = auto | on | off`, read through CFG0. `auto` follows the
  resolved dependency. `on` activates only at the admitted release, with
  provenance `explicit` when no manifest entry exists. `off` disables the whole
  vertical.

An active record sets the `ProjectCapabilitySnapshot` bit that gates the
`LanguageRow::gated("astro", …)` row (AST1 adds the row, AST1M wires the bit
in `AST1M-AC5`). When the record is not active, the Astro vertical does no
parse, fact, projection, lint or format work for that package.

Never an activation: the `.astro` extension, a path or `src/pages` layout, an
`astro.config.*` file, a declared range, a second source beside FWA1, or the
AST1G TextMate grammar.

FWA1 expresses a per-package, version-admitted `astro` activation, so the
AST0 abort condition does not apply.

## Parser decision (PAR0 row `CL11`)

| Sublanguage | Decision | Owner |
| ----------- | -------- | ----- |
| Astro carrier (fence, markup, components, fragments, expression and attribute islands, directives, element boundaries, comments) | `NewParser` (`DK4`) | `H01` `crates/verter_astro_syntax`, filled by AST1 |
| Frontmatter (TypeScript) | Reuse (`DK1`) | `CL01` OXC through `verter_parser::oxc_parse` |
| Template and attribute expressions (TSX) | Reuse | `CL01` OXC |
| Processed `<script>` (module) and `<script is:inline>` (classic) | Reuse | `CL01` OXC |
| `<style>` (CSS, SCSS, Less) | Reuse | `CL04` `verter_css_syntax` with its declared dialects |
| `set:html`, `set:text` values and `is:raw` content | opaque | carrier; never parsed as markup |

Why `NewParser` rather than a fork of the neutral HTML tokenizer:

- Astro's distinguishing structure is not WHATWG tokenization. `{…}` islands
  sit in text and attribute positions and nest markup inside JavaScript
  (`{items.map((i) => <li>{i}</li>)}`). The tokenizer must hand exact extents
  to OXC and re-enter markup inside them. Shorthand `{attr}`, spread
  `{...props}`, backtick attribute values and the `---` fence are carrier
  syntax.
- The fork source `H08` (`crates/verter_html_syntax`) does not exist at this
  head and is not an ancestor of AST1, and AST1 rescopes if its decision would
  fork the tokenizer inside HWC1.
- PAR0 `PD06` forbids a shared HTML-family parser, recovery mode or cache key,
  so a fork could not share HWC1's recovery. Astro proves its own recovery
  (`AST1-AC2`).

Lineage (PAR0 `PD04`): no source owner and no adapted upstream material; the
upstream specification is the Astro syntax reference for 7.3.8; the oracle is
the pinned grammar oracle above; the corpus is `tests/framework-astro/AST0/corpus`
with the content digest recorded in the matrix. The carrier is tooling-only:
it has no compiler backend, and a compile demand reports `UNSUPPORTED` (PAR0
`PN04`, CPF0 `PC04`/`PC06`).

### Grammar corpus

Eight valid files cover every construct AST1 names: the fence, `Props`, text
and attribute expressions, markup nested in callbacks and conditionals,
components, namespaced components, both fragment forms, shorthand, spread and
backtick attributes, every directive family (`client:*`, `server:defer`,
`set:*`, `is:raw`, `class:list`, `define:vars`, `transition:*`), processed and
inline scripts, plain and `lang="scss" is:global` styles, all three comment
forms and slots. `AST1-AC1` round-trips each byte for byte.

Three recovery files hold an unterminated `{`, an unclosed fence and an
unclosed element, each followed by identifiers whose authored offsets
`AST1-AC2` checks. The upstream implementations disagree on these inputs, so
neither is the recovery oracle.

## Hosts and the operation matrix

Hosts in ruling-1 order:

1. **tsgo** answers every TypeScript-region operation (frontmatter,
   expressions, processed scripts) through the Verter LSP's TypeProvider tsgo
   routes: `TsgoCompositeProvider` attaches shared tsgo first, then managed
   tsgo, over the AST6 companion synced by `carrier_sync.rs`, bound through
   `ProjectBinding`, and mapped back through `ProviderPositionMapper` with
   generated-only spans suppressed. No new client transport.
2. **verter-lsp** answers carrier-only operations and never a TS-region one.
3. **typescript-plugin** (tsserver) receives the carrier-generic companion
   through the existing carrier-store path, and nothing Astro-specific.
4. **verter-session** produces facts through the shared kernel; types come
   from TypeInfo and tsgo only, and a fact neither gives is
   `Unavailable(tsgoLimitation: <op>)`.

| Host | Owned cells (producer, acceptance) |
| ---- | ---------------------------------- |
| tsgo | diagnostics (AST6, `AST6-AC1`); hover, definition, references (AST5, `AST5-AC1`); component-tag references (AST5, `AST5-AC2`); island navigation (AST5, `AST5-AC5`); rename (AST5R, `AST5R-AC2`); generated-only suppression (AST6, `AST6-AC2`); `.astro` import surface (AST6, `AST6-AC3`) |
| verter-lsp | syntax diagnostics, symbols, folding and selection, carrier tokens (AST1S, `AST1S-AC1`–`AC4`); HTML element and attribute completion, slot-name completion (AST5, `AST5-AC3`); partial results after recovery (AST5, `AST5-AC4`); slot rename, unsafe-rename refusal, step-down (AST5R, `AST5R-AC1`/`AC4`/`AC5`); formatting (AST8F, `AST8F-AC1`); lint host (AST8, `AST8-AC2`); lint packs (LAS1–LAS6, each `-AC1`) |
| typescript-plugin | carrier-generic companion delivery (AST6, `AST6-AC6`) |
| verter-session | props, slots, unsupported facets (AST3, `AST3-AC1`/`AC3`/`AC4`); binding and islands (AST2, `AST2-AC1`/`AC3`); island, realm and route facts (AST4, `AST4-AC1`/`AC3`/`AC4`); style and a11y facts (AST7, `AST7-AC1`/`AC3`); forwarding, transport, render-tree and composed-page contributions (FWD1-ASTRO, TRN1-ASTRO, RND1-ASTRO, CPD1-ASTRO, each `-AC1`) |

Truthful exclusions:

- **Not received by any acceptance item yet:** TS-region completion (`C10`),
  component-tag definition to a `.astro` target (`C55`), directive-name
  completion (`C21`, also conditional on the projection not typing the
  directive) and auto-close (`C22`). The AST5 outcome names each, but no AST5
  acceptance item receives it. Each becomes claimable only through an AST5
  acceptance amendment.
- **Not claimed by this train:** TS-region signature help, inlay hints,
  document highlights, code actions and semantic tokens (`C11`), linked
  editing (`C23`) and `<script is:inline>` language features (`C13`).
- **Forbidden:** any Verter-computed TS-region answer (`C12`), any
  Astro-specific tsserver behaviour (`C36`).
- **Build and runtime (ruling 4):** no runtime compile (`C51`), no consumption
  of `astro build` or `astro dev` output or its maps (`C52`), no DBG, TST or
  WPF map (`C53`), and no execution of Astro code or config (`C54`).

Exposure through the browser (WASM) and native hosts is qualification owned
by AST9 (`AST9-AC4`); the VS Code client row is AST6's (`AST6-AC4`). Coverage
is AST9's (`AST9-AC1`) and promotion AST10's (`AST10-AC1`). Displaced routes
are recorded by AST1M (`AST1M-AC6`) and closed by AST10 (`AST10-AC4`).

No cell is a support claim at ratification. ASTP, an installed parser or
oracle, and syntax highlighting are never product evidence.

## Facets (ruling 3)

| Kind | Disposition | Provenance and source | Owner |
| ---- | ----------- | --------------------- | ----- |
| PROPS | supported | `native`: the `Props` type, types from the checker | AST3 `AST3-AC1` |
| SLOTS | supported | `derived(template)`: `<slot>`, `<slot name>` and `Astro.slots` | AST3 `AST3-AC3` |
| EMITS | unsupported | structural: no event surface | AST3 `AST3-AC4` |
| EXPOSE | unsupported | structural: no imperative handle | AST3 `AST3-AC4` |
| OPTIONS, MODEL | unsupported | not used by class B | AST3 `AST3-AC4` |

An unmapped facet is `UNSUPPORTED`, never empty. Until AST3, the descriptor
registers with every kind `UNSUPPORTED` (`AST1-AC5`).

## Wire tag

`FRAMEWORK_TAG_ASTRO = 10`, from the class-B allocation 10–14 (class A holds
6–9). REG0 pre-allocates it with disposition `DeferredVertical`; AST1 flips it
to `Registered` in the family module (`AST1-AC5`). `OPEN_CANONICAL` (5) is a
structural non-tag — `tag_disposition` returns `None` for it — and is
rejected, as are a reused value and any value in 6–9.

## Acceptance

Each acceptance item is met here by reviewed data; its planted-row rejection
proof runs at AST1G through `node --test tests/framework-astro/AST0/astro-lock.spec.ts`.

- **AST0-AC1 — pinned release:** `astro-version-lock.json` names exact
  versions with official sources; `cases.md` lists the floating, `latest`,
  unknown, beta and diverged-pin plants, including the WDX1 `astro@5.0.0` row.
- **AST0-AC2 — owned matrix:** every cell in `astro-capability-matrix.json`
  has one producer and one receiving acceptance ID or a reasoned exclusion;
  `cases.md` lists the unowned, duplicate-owner, BND-build and fabricated
  plants.
- **AST0-AC3 — activation and host policy:** `astro-activation-policy.json`
  and the matrix hosts; `cases.md` lists the extension-only, second-source,
  Verter-native TS-region, tsserver-specific and parser-authority plants.
- **AST0-AC4 — proof is not support:** every cell has `claimBasis: none` and
  the matrix's `notEvidence` list; `cases.md` lists the ASTP, installed-parser
  and highlighting plants.
- **AST0-AC5 — wire tag ratified:** the lock's `wireTag`; `cases.md` lists
  the `OPEN_CANONICAL`, reused-value and class-A plants.

Incremental equivalence and bounded work do not apply: no cache, query,
cancellation or hot path is touched, and no production byte changes.
