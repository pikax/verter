# Parser decision, ownership, reuse and lineage

This decision makes parser choice evidence-based per grammar contract, while
preventing both arbitrary parser proliferation and an omni parser. Today,
framework-shaped host/session registries and untagged public boundaries own
parser choice. The final and sole owner is the typed immutable universal catalog
and the demand-selected kernel services.

It describes the repository at `docs(arch): prove the carrier
frontend/compiler-backend split (#807)`, 2026-10-09. It follows the docs-only
rule in [README.md](README.md): it changes no production route and adds no
check. It builds on the [authority inventory](authority-inventory.md), the
[constitution](constitution.md), the [identities](identities.md), the
[catalog](catalog.md) and the
[carrier frontend/backend split](carrier-frontend-backend.md). It does not
re-own anything they assign.

## Machine-readable products

The reviewed contract data lives in `tests/kernel/PAR0/products/`:

| File | Holds |
| ---- | ----- |
| `parser-inventory.v1.json` | Decision kinds `DK1`–`DK5`, contracts `PD01`–`PD09`, current parser routes `G01`–`G13`, the grammar classification `CL01`–`CL38`, parser homes `H01`–`H09`, outcomes `PAR-O..`, consumers `PAR-C..`, plan consumers `PAR-P..`, the displaced route `PAR-D01`, referenced routes, category coverage, empty populations, findings `PAR-F..` and transferred obligations |
| `parser-case-table.v1.json` | Negative cases `PN01`–`PN12`, which UAI0 runs, positive cases `PP01`–`PP05` and work counters `WC01`–`WC03` |

Every `successorPath` starts at PAR0 and follows predecessor edges in the
controller-owned plan. UAK0's `D..`, UAK1's `L..`/`K..`, VID0's `I..`/`R..`,
CAT0's `T..`/`CR..` and CPF0's `T..`/`F-D..` rows keep their owners. PAR0
refines or references them by id.

## Method

1. Found every place that invokes a parser in production `src`, and every
   parser crate in the workspace manifests (OXC, the Vue and Svelte parsers,
   `verter_css_syntax`, JSDoc, `toml`, `serde_json`, `regex`). Each one is a
   route `G..`, recorded with its production callers.
2. Followed parse identity from `verter_language::parse_identity` to every
   cache that keys on it: `FileArtifactKey`, `CarrierParseKey`,
   `TemplateExprKey`, and the retained snapshot `SnapshotKey`.
3. Classified every grammar the plan names into one row per exact grammar
   contract. This covers carriers, embedded syntax, dialects, overlays,
   attachments, document languages and embedded DSLs.
4. Gave each row one owner: a descendant of PAR0 whose charter delivers that
   grammar. An existing route that stays is revalidated by UAI0.
5. Placed parser homes using UAK1's layers and the actual dependency graph.

## What exists at the described head

- **One JS/TS parser behind one choke point.** `oxc_parser` 0.151.0 is
  constructed only in `verter_parser::oxc_parse::Parser` (`G01`). Every
  production call site uses that wrapper. Every explicit `ParseOptions` turns
  `parse_regular_expression` off, and `oxc_regular_expression` is only a
  transitive lock entry.
- **SourceType has no single owner.** At least ten mappings pick an OXC
  `SourceType` for a script region, and they disagree on defaults (`G02`,
  `PAR-F04`).
- **Two in-house carrier parsers.**
  - Vue: the `verter_parser` tokenizer and `Syntax`, entered through
    `compile::parse_sfc` (`G03`).
  - Svelte: `verter_compiler::svelte::parser::parse_svelte` (`G04`).

  Both delegate expressions to `G01`. A `lang="pug"` template is not parsed.
- **One in-house CSS owner.** `verter_css_syntax` covers CSS and the
  SCSS/Sass/Less/Stylus dialect modules with a lossless CST sink (`G05`). The
  real preprocessing runs in the JS host as an external transform stage.
- **JSDoc is a sanctioned text path.** Tag payloads are wrapped, parsed by
  OXC and lowered to typed IR (`G06`). Three further hand-written tag scanners
  exist (`PAR-F06`).
- **Parse identity covers more than content.**
  `ParseKey = (ContentId, LanguageId, compatibility domain, compatibility epoch, SyntaxProfileId)`
  (`G07`).
  - The domain is chosen by framework spelling (UAK0 `D04`).
  - `CarrierParserGrammarVersion` enters only the grammar fingerprint
    (`PAR-F02`).
  - The closed `CarrierGrammarConfig` and its two duplicate option derivations
    are UAK0 `D01` (`G08`).
- **The retained OXC snapshot is keyed by content.** `SnapshotKey` is
  `{canonical, whole_hash, parse_env_hash}`. The `SourceType` rides beside the
  key and is read only on a cold parse (`G09`, `PAR-F01`).
- **Two text formats are read with ad-hoc tooling.**
  - TOML: only `verter_validation_probe` uses the `toml` crate in production
    (`G11`, `PAR-D01`).
  - JSON/JSONC: four in-house comment strippers feed `serde_json` (`G12`,
    `PAR-F05`).
- **Nothing else.** There is no parser for HTML, Pug, Astro, Angular, Marko,
  Glimmer, MDX, ERB, Liquid, TOML, XML, JSON5, YAML, Markdown, GraphQL, SQL,
  Cypher, DynamoDB or regex, and no third-party parser crate for them (`G13`).

## Decision kinds

A `ParserDecision` (`PD01`) is exactly one of:

| Kind | Meaning | Example rows |
| ---- | ------- | ------------ |
| `DK1` Reuse | The region's grammar contract is exactly an existing owner's. It is delegated unchanged through the embedded-language row (CAT0 `T05`). | script regions → OXC; `<style>` → CSS |
| `DK2` Dialect | A dialect hook that the owner declares and that keeps the owner's lossless tree model. It is a typed owner-local table, never a framework branch. | SCSS, Less, Sass, Stylus; JSON5; SQL dialects; MDX over Markdown |
| `DK3` ForkAndSpecialize | Copy the closest proven parser into a new home and remove the source's carrier assumptions. The fork has no edge back to its source and re-proves its own corpus and recovery. | neutral HTML from the Vue tokenizer |
| `DK4` NewParser | A dedicated parser, chosen when the closest candidate fails the pinned grammar/recovery corpus or no candidate exists. | Astro, Marko, Glimmer, ERB, Liquid, TOML, XML, YAML, Markdown, GraphQL, SQL, Cypher, DynamoDB, regex |
| `DK5` NoParser | An overlay on JS/TS/JSX/TSX, or an attachment on HTML attributes or tagged templates. It registers no frontend and declares no grammar. | React, Solid, Preact, Qwik, Stencil; Lit, Alpine, htmx, Stimulus |

**Selection is evidence-based.** Try the kinds in order: `DK1`, `DK2`, `DK3`,
`DK4`. Take the first one whose pinned grammar corpus and recovery corpus pass.
The corpus run that decided is recorded in the decision.

**A decision is not support.** It is not implementation support and not a
native-compiler commitment.

**A failed corpus means rescope.** If a vertical's closest parser fails the
pinned grammar/recovery corpus, that vertical is rescoped. The failing parser
is never stretched with carrier branches.

## Contracts

### `PD01` — `ParserDecision`

Every exact grammar contract has exactly one decision. A decision carries:

- the grammar name and exact specification edition or release;
- the `CarrierProfileId` it serves (VID0 `I03`);
- the kind;
- the owner: a home `H..`, or the existing route `G..`;
- `ParserId` and `ParserGrammarEpoch`;
- lineage, license and oracle corpus;
- recovery class and budget class;
- the delegations from region role to embedded grammar (CAT0 `T05`);
- the selecting evidence.

The decision lives in the vertical manifest's parser section (VIM0 schema,
VIM1 compiler) and is rendered onto the CAT0 `T01` carrier row.

### `PD02` — ownership key

- Ownership is keyed by `(CarrierProfileId, ParserGrammarEpoch)`, and one
  exact grammar contract has one owner.
- Never an ownership, selection or cache key: a family name, a
  `FrameworkProfileId`, an adapter spelling, a `FrameworkTag`, a file
  extension or "HTML-like" (VID0 `R03`, `R15`; CAT0 `CR10`).
- An unresolved framework name selects no parser. It returns the typed
  unsupported outcome and never falls back to the nearest carrier.
- The central grammar match that owner-local registration makes obsolete is
  UAK0 `D01`'s deletion (CPF1).

### `PD03` — reuse equality and cache keys

- **Equality.** Two parses are interchangeable only when
  `(ContentId, CarrierProfileId, ParserId, ParserGrammarEpoch, SyntaxProfileId)`
  are all equal. `SyntaxProfileId` carries the normalised parse options and
  the dialect identity.
- **No content-only reuse.** A content hash alone never admits reuse, and
  neither does content plus path.
- **No semantic profile in the key.** Syntax artifacts never key on a
  semantic profile (VID0 `R15`).
- **Exact invalidation.** A change of epoch, dialect or option misses exactly
  the affected carrier's artifacts.
- **No degraded warm entries.** A cancelled, stale or partial parse is never
  admitted as complete and never warms a cache.

### `PD04` — lineage, license and corpus

A fork or new parser records:

- **Source.** The source owner and its grammar epoch. For an in-repository
  source, its module path plus the landing title and ISO date of the source
  state. Commit SHAs, hashes and URLs are never lineage evidence.
- **Upstream.** The upstream specification or project, by name and released
  version.
- **License.** The SPDX license of any adapted upstream material.
- **Oracle corpus.** Its name, released version, checked-in path and content
  digest.

Third-party implementations (`@astrojs/compiler`, `@marko/compiler`,
`@glimmer/syntax`, `content-tag`, toml-test and the other suites) are oracles
only.

### `PD05` — lossless recovery, error, fuzz and budget

- **Lossless.** Every owned parser reproduces the input bytes exactly from its
  tokens and trivia.
- **Error-tolerant.** Every input yields a tree plus located, typed
  diagnostics; no input causes a panic or abort.
- **Exact spans.** Spans are exact UTF-8 spans (ENC0).
- **Recovery is per grammar.** Recovery is proven on the grammar's own
  malformed corpus. Two parsers share recovery semantics only with that proof
  for each.
- **Bounded.** Nesting is bounded with a typed depth diagnostic, scanning is
  linear per byte, and expansion is budgeted (for example, XML entities).
- **Fuzzing.** Fuzzing asserts no panic, termination and the round trip.
- **Timing never gates.** Wall-clock evidence is a bench-m3 evidence run only.

### `PD06` — evidence-gated HTML-family extraction

- **Neutral HTML is a fork.** The neutral HTML parser is a fork of the Vue
  tokenizer lineage into its own home `H08` (HWC1).
- **No HTML family.** There is no shared "HTML family" parser, recovery mode
  or cache key.
- **Per-carrier choice.** Astro, Marko, Glimmer and Angular templates each
  choose `DK4`, or `DK3` over `H08`, in their P0 lock.
- **Extraction is reserved.** Extracting a shared tokenizer from several forks
  needs a later decision showing that every consumer's pinned grammar and
  recovery corpora pass unchanged through the extracted owner. The extraction
  may never add a carrier branch.

### `PD07` — retained-snapshot boundary

Parser ownership keeps the engine's boundary (amendment 2026-09-30):

- `verter_semantic` discovers;
- `verter_semantic_source` lowers demanded bodies through the
  scheduler-retained snapshot;
- `verter_session_query` carries owned data;
- session and scheduler keep acquisition and execution.

It adds:

- no second parse or lowering cache;
- no engine-to-parser dependency;
- one parse per parse key;
- zero eager declaration bodies.

### `PD08` — parser homes and layers

A parser lives in one of two places:

- a dedicated `verter_<grammar>_syntax` crate (the default); or
- a dialect module of the owner whose lossless tree model it shares.

It never lives in a crate of the parser-free closure: `verter_identity`,
`verter_span`, `verter_language`, `verter_session_query`, `verter_type_engine`,
`verter_type_expr` and `verter_execution`. `verter_session_query` and
`verter_type_engine` depend on `verter_language`, so a parser there would enter
the engine's closure. It also never lives in a compiler-backend module
(UAK1 `LC`).

Placement by layer:

- Vertical carrier grammars are UAK1 `L3`.
- Neutral grammars are neutral syntax owners beside `verter_css_syntax`. These
  are HTML, Pug, the CSS dialects, the document languages and regex.
- A neutral grammar never lives in a vertical crate.

A parser crate may depend only on:

- `verter_language`, for the neutral parse-artifact DTOs;
- `verter_span`;
- the neutral owners its `DK1` rows name;
- the OXC choke point.

It never depends on a semantic profile or a compiler backend.

### `PD09` — admission policy

- **OXC.** OXC is the only JS/TS/JSX/TSX parser, reached only through the
  choke point.
- **Everything else is in-house.** Every other product parser is in-house,
  lossless and error-tolerant with exact spans.
- **No third-party parser crates.** `toml` is internal config reading only,
  until TOMLX removes it.
- **Regex.** `parse_regular_expression` stays off, and `oxc_regular_expression`
  is never a direct dependency. ECMAScript regex belongs to `verter_regex`
  (RGX1), and OXC supplies only the literal span, pattern and flags.
- **Formatters and linters own no parser.**

## Classification

| Rows | Grammar | Class | Kind | Home | Owner |
| ---- | ------- | ----- | ---- | ---- | ----- |
| `CL01` | ECMAScript/TS/JSX/TSX | script carrier and regions | Reuse (OXC) | `G01` | retained (UAI0) |
| `CL02`, `CL03` | Vue SFC, Svelte | dedicated carriers | NewParser (existing) | `G03`, `G04` | retained (UAI0); relocation `K03` (FWC1), split `F-D01` (CPF1) |
| `CL04` | CSS | neutral syntax | NewParser (existing) | `verter_css_syntax` | retained (UAI0) |
| `CL05`, `CL06` | SCSS, Less | embedded syntax | Dialect of CSS | `verter_css_syntax` | DIAL5 |
| `CL07`, `CL08` | Sass indented, Stylus | embedded syntax, dedicated frontends | Dialect of CSS | `verter_css_syntax` | DIAL6S, DIAL6Y |
| `CL09` | Pug | embedded syntax | NewParser | `H09` `verter_pug_syntax` | DIAL1 |
| `CL10` | HTML | neutral carrier | ForkAndSpecialize (Vue tokenizer) | `H08` `verter_html_syntax` | HWC1 |
| `CL11` | Astro | dedicated carrier, tooling-only | NewParser or fork of `CL10` (AST0) | `H01` `verter_astro_syntax` | AST1 |
| `CL12` | Angular templates | dedicated template carrier | fork of `CL10` or NewParser; expression grammar recorded (ANG0) | `H02` `verter_angular_syntax` | ANG1 |
| `CL13` | Marko | dedicated carrier | NewParser; reuse/fork of `CL10` recorded (MRK0) | `H04` `verter_marko_syntax` | MRK1 |
| `CL14` | Handlebars/Glimmer | dedicated carrier | NewParser or fork of `CL10` (GLM0) | `H05` `verter_glimmer_syntax` | GLM1 |
| `CL15` | MDX | carrier composed over Markdown | Dialect of `CL32`; ESM/JSX → OXC | `H03` `verter_mdx_syntax` | MDX1 |
| `CL16` | ERB | dedicated carrier | NewParser: island scanner + Ruby boundary lexer | `H06` `verter_erb_syntax` | ERB1 |
| `CL17` | Liquid | dedicated carrier | NewParser with dialect tables | `H07` `verter_liquid_syntax` | LIQ1 |
| `CL18`–`CL22` | React, Solid, Preact, Qwik, Stencil | overlays | NoParser | `CL01` | RCT1, SLD1, PRE1, QWK1, STN1 |
| `CL23`–`CL26` | Lit, Alpine, htmx, Stimulus | attachments | NoParser | codecs + `CL10`/`CL04`/`CL01` | LIT1, ALP1, HTX1, STIM1 |
| `CL27`, `CL28` | TOML 1.1, XML 1.0 + NS | document languages | NewParser | `verter_toml_syntax`, `verter_xml_syntax` | TOML1, XML1 |
| `CL29`, `CL30` | JSON/JSONC, JSON5 | document languages | NewParser; JSON5 Dialect | `verter_json_syntax` | DATA1, J5-1 |
| `CL31`, `CL32` | YAML, Markdown | document languages | NewParser | `verter_yaml_syntax`, `verter_markdown_syntax` | DATA3, DATA5 |
| `CL33`–`CL36` | GraphQL, SQL, Cypher 25, DynamoDB | DSLs | NewParser (SQL dialect hooks) | `verter_graphql_syntax`, `verter_sql_syntax`, `verter_cypher_syntax`, `verter_dynamodb_syntax` | GQL1, SQL1, NEO1, DDB1 |
| `CL37` | ECMAScript regex | DSL | NewParser | `verter_regex` | RGX1 |
| `CL38` | JSDoc tag types | comment syntax | Reuse (OXC) + tag scanner | `G06` | retained (UAI0) |

Every dedicated-grammar row carries two acceptance slots:

- an exact-version grammar slot (`grammarSlot`);
- a lossless-recovery slot (`recoverySlot`).

Both name the owning vertical's receiving acceptance ID; for example
`AST1-AC1`/`AST1-AC2` or `TOML1-AC1`/`TOML1-AC2`. The P0 locks (AST0, ANG0,
MDX0, MRK0, GLM0, ERB0, LIQ0) fill the open kind choices of their rows in their
owned matrix (`AC2`). The overlay and attachment locks record "no parser".

## Parser homes

REG0 creates `H01`–`H07` as empty skeleton crates under `crates/`, with their
`[workspace.dependencies]` entries (`REG0-AC7`). The workspace's `crates/*`
glob makes them members.

- **Dependents.** Only the family's vertical modules (today the `verter_session`
  `framework/families/<family>` vertical half and `verter_lsp`'s
  `frameworks/<family>`) and the composition root may depend on a home.
- **No module under `verter_language`.** `crates/verter_language/src/carriers/`
  is not a parser home (`PAR-F07`).
- **Neutral homes.** HWC1 creates `H08` (neutral HTML) and DIAL1 creates `H09`
  (Pug).

## Displaced and referenced routes

| Route | Category | Unit | Owner |
| ----- | -------- | ---- | ----- |
| `PAR-D01` | parser admission | `toml` as a production dependency of `verter_validation_probe` | TOMLX (`TOMLX-AC1`) |

| Category | Recorded here | Referenced |
| -------- | ------------- | ---------- |
| central framework switch | — | `D01`, `D04`, `D07` (CPF1); `K03` (FWC1); `F-D01` (CPF1) |
| untagged coordinate/public identity | — | `D08` (CPF1) |
| duplicate component information authority | — | `F-D06` (CPF1); `D12` (TIF1) |
| parser admission | `PAR-D01` | — |

Each central grammar match at this head already has an owner. PAR0 adds the
rule (`PD02`) and the negatives that keep a new one out (`PN03`, `PN06`,
`PN08`).

## Outcomes and consumers

| Id | Outcome or consumer | Owner |
| -- | ------------------- | ----- |
| `PAR-O01` | `ParserDecision` manifest section and typed catalog rendering | VIM1 (`VIM1-AC2`) |
| `PAR-O02` | Parser homes `H01`–`H07` with manifest edges | REG0 (`REG0-AC7`) |
| `PAR-O03` | Ownership validator and negatives `PN01`–`PN12` | UAI0 (`UAI0-AC-R1`) |
| `PAR-O04` | One resolved owner per grammar contract; `CL01`–`CL04`, `CL38` revalidated | UAI0 (`UAI0-AC1`) |
| `PAR-O05` | Neutral HTML fork with lineage and no edge back | HWC1 (`HWC1-AC1`) |
| `PAR-O06` | Recovery snapshot keyed by carrier/parser identity and grammar epoch | LSO1C (`LSO1C-AC3`) |
| `PAR-C01` | OXC choke point and script-region parses | UAI0 (`UAI0-AC1`) |
| `PAR-C02` | Parse identity and artifact caches: reuse rejection verified | PER0 (`PER0-AC2`) |
| `PAR-C03` | `RecoverySnapshot`, `NativeSyntaxDiagnostic` | LSO1C (`LSO1C-AC3`) |
| `PAR-C04` | Family carrier-grammar rows, owner-local | REG0 (`REG0-AC7`) |
| `PAR-C05` | Probe-manifest TOML reader | TOMLX (`TOMLX-AC1`) |
| `PAR-C06` | Compiler request and stage identities consume the admitted parse | CMP1 (`CMP1-AC2`) |
| `PAR-C07` | Neutral HTML fact authority | HWC1 (`HWC1-AC1`) |
| `PAR-C08` | Regex literal hand-off from OXC | RGX1 (`RGX1-AC1`) |

The plan consumers `PAR-P01`–`PAR-P15` list which successor reads which rows.

## Findings recorded for the receiving owners

- **The retained snapshot reuses by content** (`PAR-F01`).
  - `SnapshotKey` carries no parser identity or `SourceType`, and a warm hit
    ignores the requested `SourceType`. `PD03` forbids this shape; no defect
    has been reproduced.
  - No PAR0 descendant charters this key. It is routed to UAI0's drift return,
    which runs `PN01` against it. Operator question `par0-unowned-parse-routes`
    asks who owns the cutover.
- **Two grammar-version notions** (`PAR-F02`, CPF1). `ParseKey` uses
  per-domain compatibility epochs, while `CarrierParserGrammarVersion` enters
  only the fingerprint. `PD03` needs one `ParserGrammarEpoch` in the key.
- **LSO1C keys `RecoverySnapshot` by `FrameworkProfileId`** (`PAR-F03`). A
  recovery snapshot is a syntax artifact. It keys by carrier and parser
  identity (VID0 `R15`).
- **Scattered `SourceType` mappings** (`PAR-F04`, CPF1). The target is one
  neutral mapping from the region's embedded `CarrierProfileId` (CAT0 `T05`).
- **Four JSONC comment strippers** (`PAR-F05`). No descendant charters their
  cutover onto `CL29`. They take the same question and route as `PAR-F01`.
- **Three hand-written JSDoc tag scanners** (`PAR-F06`, NCF-JD-JSDOC).
- **REG0, LIQ1 and ERB1 place grammar work in `verter_language`** (`PAR-F07`).
  - REG0 pre-registers `verter_language/src/carriers/` "when PAR0 places them
    there". PAR0 does not, so `H01`–`H07` replace that location.
  - LIQ1 and ERB1 add central `CarrierGrammarConfig` arms and a private CST in
    `verter_language`. Both move to owner-local rows (CAT0 `CAT-F03`) and to
    their homes.
- **TOMLX misses one manifest** (`PAR-F08`). TOMLX names four `toml`
  manifests, but there are five: `verter_source_policy_gate` declares it as a
  dev-dependency, and `cargo tree` counts dev edges.
- **HWC1 and DIAL1 name the wrong homes** (`PAR-F09`). HWC1 names
  `verter_language/src` for the HTML parser, and DIAL1's conflict domain names
  `verter_parser` for Pug. `PD08` places them in `H08` and `H09`.

## Acceptance evidence

This change adds contract text and data only, so existing coverage and bounded
inspection are the right evidence. The diff adds no test.

- **AC1 — ownership contract.**
  - The inventory binds every outcome, consumer, classification row, home and
    the one displaced route to one existing plan node, a successor path from
    PAR0 and a receiving acceptance ID.
  - UAK0, UAK1, VID0, CAT0 and CPF0 rows are referenced, not re-owned.
  - The executable validator and the negatives belong to UAI0
    (`UAI0-AC-R1`).
- **AC2 — positive contract.** Existing coverage pins the identity,
  provenance and ordering of the boundaries named here:
  - in `crates/verter_language/tests/cases/parse_identity.rs`:
    `parse_key_canonical_bytes_and_digest_are_pinned`,
    `syntax_profile_canonical_bytes_and_digest_are_pinned`,
    `identical_inputs_share_parse_keys_and_parse_affecting_changes_do_not`
    and `vue_parse_affecting_options_change_the_syntax_profile`;
  - the `verter_language` `carrier_grammar` unit tests
    `every_canonical_grammar_input_discriminates_the_fingerprint` and
    `grammar_fingerprint_is_stable_across_authority_lifetimes`;
  - `crates/verter_compiler/tests/cases/parse_diagnostic_determinism.rs`;
  - `no_direct_oxc_parser_calls_outside_scheduler_path` in `verter_session`;
  - `indexed_ready_publish_lowers_zero_decl_bodies`,
    `lazy_decl_body_singleflight_lowers_once` and
    `lazy_decl_lowering_uses_scheduler_snapshot_not_reparse` in
    `verter_session` for `PD07`;
  - `framework_registry_complete` in `verter_session` for `PN06`.

  New and extended tests belong to UAI0 (`UAI0-AC-R1`).
- **AC3 — incremental equivalence: not applicable.** No cache, cancellation,
  stale-publication or partial-result authority is touched, and no production
  byte changes. `PD03` and `PN01` bind "content-only reuse is rejected" to
  UAI0, and `PP05` binds exact grammar-epoch invalidation to PER0.
- **AC4 — bounded work: not applicable.** No hot path changes. `WC01`–`WC03`
  name the existing parse-once and lowering readers that the receiving owners
  report.
