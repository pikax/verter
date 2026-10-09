# MDX with React: delivery and exact-version contract

This decision fixes what "MDX with React" means for Verter before any MDX code
exists: the exact release set, when MDX with React is active, which host
answers each operation and who produces it, which plugin syntax is admitted,
how the `MDXContent` module maps onto the framework facets, and the wire tag.
Every later MDX node, and the cross-train MDX successors, build against it.

It describes the repository at `docs(arch): record the parser decision,
ownership, reuse and lineage (#812)`, 2026-10-09. It follows the docs-only rule
in [kernel/README.md](kernel/README.md): it changes no production route and
adds no check. It builds on the web-product constitution (WDX0)
(claim and routing law), the [parser decision](kernel/parser-ownership.md)
(`CL15`) and the [identities](kernel/identities.md) (one release per manifest),
and re-owns nothing they assign.

## Machine-readable products

The reviewed contract data lives in `tests/framework-mdx/MDX0/`:

| File                                  | Holds                                                                                                                                                                                               |
| ------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `products/mdx-version-lock.json`      | The exact MDX release, the React profile reference, next-major rule and excluded selectors                                                                                                          |
| `products/mdx-activation-policy.json` | The FWA1 activation row (`A1`–`A4`), the never-activates rows (`N1`–`N6`) and the host policy                                                                                                       |
| `products/mdx-capability-matrix.json` | Cells `C01`–`C42` (operation, host, profile, producer, acceptance), exclusions `X01`–`X12`, plugin syntax, overridable constructs, feature sources, parser decisions, facets, wire tag and baseline |
| `cases.md`                            | Every planted row and the reason it must fail                                                                                                                                                       |
| `manifest.json`                       | The product index and the validator's owner                                                                                                                                                         |

The validator is not part of this decision. MDX1G adds `contract.ts` and
`mdx-lock.spec.ts` (`MDX1G-ACV`) and implements every row of `cases.md`
against these products; REG0's `scripts/run-framework-locks.mjs` discovers it.
Until then the products are data that REG0, FWA1 and FCH1 read.

## Baseline

- No `.mdx` row in `crates/verter_language/src/registry.rs`, and no MDX
  `CarrierGrammarConfig` in `crates/verter_language/src/carrier_grammar.rs`.
- `FrameworkTag` has no MDX entry (`typeinfo.proto` ends at
  `FRAMEWORK_TAG_OPEN_CANONICAL = 5`).
- `tests/sfc-projection/STP7/products/execution-context-boundary-contract.json`
  lists `mdx` under `forbiddenFrontends`. MDXP is a receiving obligation, not
  support.

## Decisions

### 1. Release

| Package         | Pin      | Role                                                                           |
| --------------- | -------- | ------------------------------------------------------------------------------ |
| `@mdx-js/mdx`   | `3.1.1`  | Official compiler; a test-only conformance oracle, never run by a product path |
| `@mdx-js/react` | `3.1.1`  | The React provider (`MDXProvider`, `useMDXComponents`)                         |
| `@types/mdx`    | `2.0.14` | `MDXProps`, `MDXContent`, `MDXComponents`, `MDXModule`                         |

Each pin names its npm registry source and integrity. React is not pinned
here: the one React 19.x profile is RCT0's
`tests/framework-react/RCT0/products/react-version-lock.json`, referenced by
path and profile name. There is never a second React lock; MDX1M-AC6 adds the
pin-equality assertion. A next MDX major is admitted only once announced and
ratified by an MDX0 amendment. No canary, prerelease or legacy (MDX 1/2) pin is
admitted, and admission reads the resolved installed version.

### 2. Activation

FWA1's `FrameworkActivation` record is the only source. MDX with React is
active for a file only when all four hold:

- `A1` — `mdx` is active at an admitted MDX release (PM-resolved `@mdx-js/mdx`).
- `A2` — React is active at RCT0's admitted release (RCT0's FWA1 row).
- `A3` — the MDX `jsxImportSource` resolves to `react`: a per-file
  `@jsxImportSource` pragma, else a statically declared option read as data,
  else the `@mdx-js/mdx` 3 default (`react`).
- `A4` — the `providerImportSource` resolves to the installed `@mdx-js/react`.

A `.mdx` extension alone, a Preact or Vue `jsxImportSource`, a Markdown-only
project and an option known only by executing a config never activate it. The
extension selects the carrier row only; the row stays gated until FWA1 admits
`mdx`. No activation rule reads a file name, so the charter's abort condition
does not apply. `frameworks.mdx = off` disables the vertical and makes it do
zero work (MDX1M-AC5).

### 3. Host

tsgo has replaced tsserver.

- Inside `.mdx` ESM, JSX and expression regions, TS answers are tsgo's over
  the MDX6 projection, through `TsgoCompositeProvider`
  (`crates/verter_lsp/src/tsgo/composite.rs`) with companion sync and
  `ProjectBinding`. Types are read through `crates/verter_tsgo_api`
  (`TypeProviderKind::Tsgo`). Verter only remaps, through
  `ProviderPositionMapper`, and suppresses generated-only spans.
- `.tsx` importers see `.mdx` modules through the companions tsgo opens
  directly — the carrier route `crates/verter_lsp/src/background_drain.rs`
  already uses.
- The Verter LSP adds MDX-only enhancements: native syntax diagnostics, the
  outline, folding and selection ranges and carrier tokens (MDX1S), provider-slot
  navigation and document symbols for exports and the layout (MDX5),
  framework-only rename positions through LSO8 plans (MDX5R), Verter lint
  diagnostics (MDX8) and formatting (MDX8F, MDX8F-MD). Each steps down through LSPX11's
  `owns(capability, position)`.
- `packages/typescript-plugin` gets compatibility only: its existing
  carrier-store reader resolves `.mdx` imports for tsserver users from the MDX6
  publication. It gains no MDX code and no MDX decoration.

No cell has Verter compute a TS answer.

### 4. Plugin syntax

Declared syntax is data; Verter never executes remark, rehype or recma
plugins. Admitted:

- `P01` GFM (tables, strikethrough, task lists, footnotes, autolink literals),
  as declared by `remark-gfm` `4.0.1`, parsed by the Markdown owner (`CL32`).
- `P02` YAML frontmatter, as declared by `remark-frontmatter` `5.0.0`, parsed
  by the YAML owner (`CL31`).

Frontmatter exposed as an ESM export, and every other plugin, are not
admitted; the nodes they would affect are `unknown` (MDX4-AC3).

### 5. Facets of `MDXContent`

| Facet  | Records              | Provenance                         | Result                       |
| ------ | -------------------- | ---------------------------------- | ---------------------------- |
| props  | `components`         | `native` (`@types/mdx` `MDXProps`) | proven                       |
| props  | `props.<name>` reads | `derived(props-read)`              | conditional; the set is open |
| events | —                    | —                                  | UNSUPPORTED                  |
| slots  | —                    | —                                  | UNSUPPORTED                  |
| expose | —                    | —                                  | UNSUPPORTED                  |

`MDXContent` has no events, slots or imperative handle, so those facets are
UNSUPPORTED, never complete-and-empty. Components used inside MDX keep RCT3's
React facets. MDX3 produces the facets (MDX3-AC2).

### 6. Wire tag

`FRAMEWORK_TAG_MDX = 14`, from the framework-common allocation. It lands with
the MDX adapter descriptor at MDX3 (MDX3-AC5). MDX1 registers only the grammar
and the gated language row.

### 7. Build

Verter does not compile MDX and does not consume official build output. MDX6
is the TS-host projection only; no BND entry, compile request or bundler hook
exists for `.mdx` (MDX6-AC4), and no DBG/TST/WPF map is promised over MDX
build output.

## Parser decisions

| Sublanguage           | PAR0 row | Decision                                                                                        | Owner |
| --------------------- | -------- | ----------------------------------------------------------------------------------------------- | ----- |
| MDX carrier           | `CL15`   | `DK2` Dialect of `CL32` through the Markdown owner's extension hook; `crates/verter_mdx_syntax` | MDX1  |
| Markdown              | `CL32`   | `DK1` Reuse                                                                                     | DATA5 |
| ESM, expressions, JSX | `CL01`   | `DK1` Reuse (OXC; extents from OXC)                                                             | UAI0  |
| YAML frontmatter      | `CL31`   | `DK1` Reuse                                                                                     | DATA3 |

There is no second Markdown parser.

## Operation matrix

One profile is admitted, `mdx-react`. Every cell has one producer and one
receiving acceptance; the products file is authoritative.

| Cells               | Operations                                                                              | Host                              | Producer |
| ------------------- | --------------------------------------------------------------------------------------- | --------------------------------- | -------- |
| `C01`               | activation record                                                                       | session                           | FWA1     |
| `C02`, `C06`, `C07` | activation gate, exact region maps, JSX-source scoping                                  | session                           | MDX1M    |
| `C03`–`C05`         | MDX 3 precedence, OXC extents, lossless recovery                                        | session                           | MDX1     |
| `C08`, `C09`        | component binding, override slots                                                       | session                           | MDX2     |
| `C10`–`C12`         | module facets, use-site surface, wire tag                                               | TypeInfo / wire                   | MDX3     |
| `C13`–`C16`         | children mode, override resolution, plugin regions, layout                              | session                           | MDX4     |
| `C17`–`C20`, `C33`  | projection; hover/definition, importer surface, diagnostics; tsserver import resolution | session / tsgo / TS-plugin compat | MDX6     |
| `C21`, `C22`        | reference remap, completion without duplicates                                          | tsgo                              | MDX5     |
| `C23`–`C25`         | provider-slot definition, partial results, document symbols                             | LSP enhancement                   | MDX5     |
| `C26`–`C28`         | tag-pair rename, typed refusals, step-down                                              | LSP enhancement                   | MDX5R    |
| `C29`               | style and accessibility facts                                                           | session                           | MDX7     |
| `C30`–`C32`         | lint diagnostics, code actions, explicit unsupported                                    | LSP enhancement                   | MDX8     |
| `C34`–`C38`         | syntax diagnostics, outline, folding and selection, carrier tokens, gating/incremental  | LSP enhancement                   | MDX1S    |
| `C39`–`C41`         | carrier view formatting, children mode kept, Markdown verbatim and malformed islands    | LSP enhancement                   | MDX8F    |
| `C42`               | Markdown content formatting                                                             | LSP enhancement                   | MDX8F-MD |

Exclusions `X01`–`X12` record build, maps over build output, runtime, plugin
execution, TS-plugin decoration, tsserver forwarding, Markdown structure
navigation (DATA6), React-only navigation (RCT5), the unsupported facets,
non-React profiles, unadmitted releases, and MDX1G's client grammar and
language configuration (highlighting is never a product operation).

Component completion (`C22`) offers a component auto-import candidate only for
an export proven to be a React component; a capitalised name is never proof,
and generic MDX parses and binds without React.

Three cells name a receiving AC that does not yet discriminate their operation:
`C20` (in-`.mdx` diagnostics, MDX6), `C22` (the component-candidate rule,
MDX5) and `C25` (export and layout symbols, MDX5). Each carries an
`acceptanceGap` with its discriminating case, routed to the producer through the
controller; the named receiving AC holds until that amendment is ratified.

## Overridable constructs and feature sources

The constructs a provider or `components` prop can substitute are the closed
set of the pinned table of components (`docs/table-of-components.mdx` at
`@mdx-js/mdx` `3.1.1`): `a`, `blockquote`, `br`, `code`, `em`, `h1`–`h6`,
`hr`, `img`, `li`, `ol`, `p`, `pre`, `strong`, `ul`; with GFM (`P01`) also
`del`, `input`, `section`, `sup`, `table`, `tbody`, `td`, `th`, `thead`,
`tr`. `wrapper` is the layout slot. MDX2 records slots, MDX4 resolves them and
MDX6 projects them only for these names.

MDX9's feature sources are the mdxjs.com documentation examples (the `docs/`
tree of `mdx-js/mdx` at the `3.1.1` tag) and that repository's
`packages/mdx/test` fixtures, both under the repository's MIT licence.

## Proof is not support

No cell cites MDXP, MDXR0, STP7, an installed parser or syntax highlighting
(the MDX1G TextMate grammar) as product evidence. A cell is evidenced only by
its producer's acceptance; MDX9-AC1 joins every cell to a passing fixture and
MDX10 closes the terminal against this matrix.
