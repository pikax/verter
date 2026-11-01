# Dialect syntax, preprocessing and mapping contract

This contract inventories the implementation described by **feat(session):
reserve module homes for the lint, css, a11y, formatter, (#1059)**,
2026-11-01. It adds contract data only. It neither enables a dialect nor
certifies upstream support. Read it with the shared
[parser ownership](../../../docs/arch/kernel/parser-ownership.md),
[embedded codec](../../../docs/arch/kernel/embedded-codecs.md) and
[formatter](../../../docs/arch/kernel/formatter.md) contracts.

The problem is claim conflation: a renderer, structural parser or formatter
can work without providing authored bindings, references or safe edits. The
intent is separate capability cells for each exact syntax, operation and host.
The sole dialect owner contributes syntax, authored origins and facts; existing
semantic, CSS, formatter and edit owners consume those contributions. No second
parser, CSS printer, semantic resolver or map composer is authorized.

## Products and claim rules

[The inventory](products/dialect-inventory.v1.json) records all six dialects,
production consumers, dependency sources and successor responsibilities.
[The case table](../evidence/DIAL0/cases.md) specifies discriminating future
proof and cites current evidence. These are reviewed data, not a new gate or
an executable support manifest. The controller retains plan/readiness state.

A capability cell is `(dialect grammar/version, parser epoch, syntax profile,
operation, host/profile, source/config/build basis, result class, evidence)`.
An unpinned workspace tool is **unqualified**, never implicitly the lockfile
version. A dependency version is installation evidence, not grammar conformance.
No current cell below claims full authored language intelligence.

| Dialect                   | Authored syntax/recovery today                                                  | Authored facts today                                                                     | Format view today                                       | External transform today                                           | Precise authored operations                                                                 |
| ------------------------- | ------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ------------------------------------------------------- | ------------------------------------------------------------------ | ------------------------------------------------------------------------------------------- |
| Pug                       | No native parser; raw non-HTML template                                         | No native Pug binding facts                                                              | No Pug view/printer                                     | Optional `pug.render` → HTML, no map returned                      | Unsupported by this contract; generated HTML is insufficient                                |
| SCSS                      | CSS owner's dialect module, lossless structure; corpus coverage remains bounded | Native structural selector/declaration facts; evaluation-dependent values stay uncertain | No dedicated dialect view established by this inventory | Vite `preprocessCSS`, or optional Sass outside Vite                | Only individually evidenced current CSS operations; no full variable/mixin resolution claim |
| Less                      | CSS owner's dialect module, lossless structure; corpus coverage remains bounded | Native structural selector/declaration facts; evaluation-dependent values stay uncertain | No dedicated dialect view established by this inventory | Vite pipeline; no direct non-Vite Less branch                      | Same restriction; generated selectors cannot become authored declarations                   |
| Indented Sass             | CSS owner's layout-capable dialect frontend                                     | Native structural facts without external compiler                                        | No dedicated dialect view established by this inventory | Vite or optional Sass; VS Code qualified Sass diagnostics          | No inferred variable/mixin intelligence from generated CSS                                  |
| Stylus (`stylus`, `styl`) | CSS owner's layout-capable dialect frontend                                     | Native structural facts without external compiler                                        | No dedicated dialect view established by this inventory | Vite; VS Code optional workspace Stylus; no direct non-Vite branch | No inferred optional-syntax binding intelligence from generated CSS                         |
| Custom PostCSS syntax     | Not a single grammar; `postcss` is not native CSS admission                     | Host reports unknown/processed-content-required; no native CSS facts                     | No generic custom-syntax view                           | Configured Vite/PostCSS pipeline only, provider-specific           | Unsupported until exact provider, syntax, map and operation are qualified                   |

Current Rust LSP formatting (`features/formatting.rs::format_document`) adjusts
SFC whitespace; it explicitly delegates block content formatting to client
tools. That is not a dialect format view. Neither TextMate Pug/PostCSS language
registration nor the VS Code CSS service's `postcss` → CSS selection establishes
a native custom grammar. The direct VS Code SCSS/Less service uses verbatim
block content; its capabilities are host-local, not Rust parser evidence.

## Consumer routes and surviving owners

| Current route                                                                                   | Surviving authority and concrete disposition                                                                                                                                                                                                                                                                                                                                          |
| ----------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `verter_css_syntax/src/dialect/{scss,less,sass,stylus}.rs`, `dialect.rs`                        | Retain the one syntax owner and `CssDialect` spelling table. DIAL5 extends SCSS/Less contributions; DIAL6S and DIAL6Y extend the separate layout frontends. Do not copy spelling or grammar tables into consumers.                                                                                                                                                                    |
| `verter_session` style analysis and `verter_semantic` style consumers                           | Consume `StyleSyntaxIr` facts; DIAL8 contributes dialect symbols through the reserved `css_symbols/dialects` module. Existing CSS semantic/query authorities keep resolution.                                                                                                                                                                                                         |
| `verter_compiler` Vue/Svelte style lowering                                                     | Retain CSS plans and `QualifiedStyleResult` stages. Compile plain CSS after external transformation; do not use generated CSS as authored symbol inventory.                                                                                                                                                                                                                           |
| `packages/unplugin/src/core/preprocessor.ts`, `style-preprocess-worker.mjs`                     | Retain external compiler orchestration. STP38 owns transform identity; DIAL7 adapts custom syntax/stages, using EMB0I map composition. Do not replace Sass/Less/PostCSS engines.                                                                                                                                                                                                      |
| `packages/vue-vscode/src/css/{cssService,transpiler,preprocessorResolver,styleDocumentText}.ts` | Retain qualified producer diagnostics and verbatim direct-language documents. Sass/Stylus output is not reparsed as authored CSS. DIAL6SF/DIAL6YF own Sass/Stylus facts, DIAL6 qualifies integration, and DIAL8 cuts consumers over; DIAL7 owns custom-stage integration; this decision does not broaden the adapter.                                                                 |
| `verter_compiler/src/compile/mod.rs` non-HTML template skip and unplugin Pug rendering          | DIAL1 owns new neutral `verter_pug_syntax` (PAR H09); DIAL2 lowers facts with EMB0I origins; DIAL3 routes Vue operations through existing Vue/LSO owners. Retire the non-HTML unsupported branch only for the exact newly supported Pug profile after integration proof; retain explicit unsupported results elsewhere. External rendering stays compilation, never an offset oracle. |
| `verter_lsp/src/features/formatting.rs` and client formatting pipeline                          | DIAL4 provides Pug views/printer contributions to the existing formatter owner, not another service. DIAL8 hands CSS-family contributions to existing FMTCS0/FMTCL0 views and retires duplicate parsing adapters. DIAL4L owns Pug lint contributions separately.                                                                                                                      |
| `packages/vue-vscode/package.json` language scopes and `docs`/public capability consumers       | Retain highlighting as highlighting. Producer AC4 delivery qualifies each operation in VIM/DX and DOC1 examples before promotion; no global support flag.                                                                                                                                                                                                                             |

**Deletion set for this patch: empty.** This is an additive constitution, with
no runtime cutover or replacement API. All current routes remain with their
owners above. Later implementations must inventory then-current routes and
remove displaced private decoders, map composition, dialect tables or parsers
in the same cutover after equivalence; an indefinite adapter is not allowed.
The current `formatter` and `css_symbols/dialects` module reservations are
implementation homes, not evidence of delivered views or symbols.

## Typed contributions, identity and map admission

The future boundary uses PAR's `ParserDecision` and lossless parse artifact,
EMB0I's `EmbeddedTextCodec`/`AuthoredMapChain`, and the existing style boundary
`StyleSyntaxIr`, `StyleStage`, `QualifiedStyleResult`, `StyleDependency` and
`StyleDiagnostic`. These are contracts to consume, not types introduced here.
Dialect facts feed canonical semantic facts; views feed the shared formatter.
Pug expansions retain lexical contexts and holes. Includes use their actual
source unit/base URI with attachment context; a mixin expansion may have many
origins and may be noneditable. No regex-to-HTML offset approximation.

SourceUnitId/RegionId lineage remains distinct from source/content/map revision.
The current CSS grammar/event epoch is `CssSyntaxGrammarVersion::CURRENT = 1` in `verter_css_syntax/src/version.rs`; this is distinct from a language release. Parser identity, grammar epoch and syntax options select syntax artifacts;
semantic attachments additionally observe their profile/project context.
Transform stages observe exact producer version, configuration, dependencies,
input/output content and build/session basis. A change of observed configuration
or included source invalidates dependent results, not unrelated regions.

Each chain edge explicitly records exact, ambiguous, synthetic or unavailable
mapping. Source Map V3 presence alone does not imply a reversible editable
range: validate bounds, coordinate domains, endpoints and revisions through the
shared map authority. Compose authored → syntax → transformed → compiled maps
without skipping missing edges. Navigation can report multiple origins; edits
require an exact reversible authored target and the existing atomic edit
transaction. Missing/ambiguous/stale maps refuse that edit, without inventing
nearest offsets. Static authored parsing remains usable without running tools.

## Delivery and proof obligations

- **DIAL0-AC1:** the matrix and route inventory assign separate parser, fact,
  view and external transform authorities. DIAL1/2/3/4/4L, DIAL5, DIAL6S,
  DIAL6Y, DIAL6SF, DIAL6YF, DIAL7 and DIAL8 implement their named boundaries; DIAL6 qualifies the two fact producers; they do not create
  another query service. The train conformance checkpoint audits actual outputs.
- **DIAL0-AC2:** the case table's generated-only and formatter-only negatives
  forbid full authored claims. Current native/unknown admission and generated
  CSS refusal tests are narrower evidence, not Pug integration proof.
- **DIAL0-AC3:** no state, cache, query, scheduling, cancellation or map code
  changes here, so runtime incremental testing is not applicable to this diff.
  Producers' AC3 must prove fresh/incremental equality, edit/revert, include
  invalidation, stale/partial/cancelled non-warming and deterministic ordering
  under perturbed discovery/scheduling. EMB0I retains primitive map proof;
  dialect producers retain integration proof on the exact profile.
- **DIAL0-AC4:** each producer's AC4 supplies operation cells for parse/recovery,
  diagnostics, completion, hover, definition, references, rename/edits,
  formatting and runtime compilation independently. Publish exact engine and
  host/profile evidence in VIM/DX and DOC1-tested `examples/reference`, with
  unsupported/partial cases, permissions, uncertainty and migration notes.
  Static syntax proves no runtime capability. Runtime compilers/plugins require
  explicit trust, real execution, teardown and source/config/build/session
  provenance; workspace package resolution never grants trust. A terminal
  checks delivered artifacts rather than these specifications.
- **DIAL0-AC5:** no hot path, counter or latency/allocation/RSS budget changes.
  Later measurements follow the standing methodology: same-worker interleaved
  baseline/candidate answers and work counts/growth ratios; timing and memory
  are advisory. Include plain CSS/HTML and unrequested-region controls, count
  parsing/mapping per observed stage, and retain the shared one-parse lifecycle.

No grammar release beyond the inventory's exact installed dependencies has
been proven here. Parser producers must pin their specification/oracle corpus,
license and grammar/recovery evidence under PAR PD04/PD05 before claiming a
version. Custom syntax qualification is per provider/version/options, never all
PostCSS plugins. Missing trust, provider or mapping is a truthful partial result;
it does not discharge a producer's required integration work.

## Requirement justification

These refine existing charter obligations rather than adding mechanisms.

| Requirement                                                        | Concern     | Prevents (concrete failure/likelihood)                                                                                       | Not covered because                                                                                     |
| ------------------------------------------------------------------ | ----------- | ---------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| Separate operation/host cells (AC1/AC2/AC4)                        | correctness | A working renderer or formatter being advertised as authored rename support; plausible when one support flag represents both | Dependency installation and compile tests do not exercise authored references or edits                  |
| Shared staged origins and reversible edit admission (EMB/PAR, AC3) | correctness | Editing a guessed mixin/include or generated selector position; plausible for many-to-many output                            | Successful transform output does not prove origin invertibility or freshness                            |
| Explicit runtime trust and provenance (charter runtime clause)     | security    | Executing a workspace plugin while claiming static parsing; plausible when tools are auto-resolved                           | Parser tests and host-neutral CI do not establish user permission or loaded workspace producer identity |
| Existing work methodology and controls (AC5)                       | performance | Counting only original bytes while transform expansion multiplies work; plausible for mixins                                 | Functional equality alone does not count intermediate stage work                                        |
