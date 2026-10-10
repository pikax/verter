# Native formatter ownership, composition and compatibility

This decision locks full-document native formatting before printer implementation.
The current owner is framework-shaped host/session registries and untagged public
boundaries; the final owner is the typed immutable universal catalog and
demand-selected kernel services. It describes `docs(arch): define embedded codecs
and exact authored map ownership (#838)`, 2026-10-10. It follows the
[docs-only rule](README.md): no production change, executable fixture, validator,
or CI lane is added. This is a required architecture contract, not a claim that
native formatting is shipped.

It consumes [parser ownership](parser-ownership.md),
[embedded codecs](embedded-codecs.md), [configuration](configuration.md), and
[coordinate domains](coordinates.md). It preserves the owners of
[identities](identities.md), [catalog](catalog.md),
[demand](demand-activation.md), and [TypeInfo](typeinfo-facade.md).

## Reviewed products and completeness

The reviewed data under `tests/kernel/FMK0/products/` is:

| File                           | Population                                                                                                                                                      |
| ------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `formatter-inventory.v1.json`  | Outcomes, implementation consumers, direct contract consumers, inherited named boundaries, current routes, deletion references and empty populations            |
| `formatter-case-table.v1.json` | Architecture, compatibility, recovery, composition, edits, maps, range, cursor, identity and bounded-work cases; existing evidence and future executable owners |

Each owned outcome and consumer has one owner, a successor path from FMK0, and
one receiving acceptance. An unnumbered receiving clause is named by its exact
heading and text, never a fabricated acceptance ID. Inherited boundaries/routes
reference the original inventory and its original rooted path: they are not
reassigned to a formatter descendant. The conformance checkpoint is read-only.
UAP0 subblock 3 owns architecture fixtures; `UAP0-AC-R1` owns executable inventory
validation and rejects missing members, unknown/pathless owners and conflicting
assignments. FMT0 owns the exact option/cell/corpus catalogs and gate wiring.
Later implementation nodes prove their production behavior and deletion; none is
proved by this data.

The census examined formatter entry points in the language, session, protocol,
identity, LSP, NAPI, WASM and MCP sources, repository manifests, and their current
callers. Generic Rust `format!`/`Display` implementations, compiler code emission,
TypeInfo text presentation and JSON pretty serialization are not document
formatting routes. They keep their own owners.

## Current routes and cutover

The live document route is
`LanguageServer::formatting` in `crates/verter_lsp/src/server/mod.rs` →
`handle_formatting` in `server/aux_features.rs` → `format_document` in
`features/formatting.rs`. The handler reads the current document and its projected
carrier blocks. The body adjusts whitespace-only gaps, trailing whitespace and
EOF newline; it ignores `FormattingOptions` and does not print block content.
Its comments suggest client-side external formatting, but there is no production
Prettier/dprint call in this route. That description is superseded by this native
contract; this decision leaves the live behavior unchanged.

FMT3 alone switches `handle_formatting` to FMT4L and deletes the old
`format_document` body and route-only helpers/tests (`FMT3-AC1`, `AC3`). FMT4L
lands dormant first. `document_formatting_provider` stays continuously advertised
through cutover. FMT4 proves promotion and deletes nothing.

`LanguageServer::on_type_formatting` → `handle_on_type_formatting` and
`document_on_type_formatting_provider` remain the existing markup tag-auto-close
feature. They are outside the formatter migration, retain the `>` trigger, and
must not call the formatter service (`FMT3-AC5`). Current LSP range formatting and
cursor results do not exist. No native formatter service, formatter DTO or
NAPI/WASM/MCP document-formatting entry point exists in the described source.
Adding one follows its named adapter owner; an unknown discovered live route
requires the FMT0 route inventory to be amended before cutover.

The charter's three inherited deletion categories remain explicit:

| Category                                  | Original routes and sole deletion/rejection owner                                                                                                                       |
| ----------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Central framework switch                  | UAK0 `D01`–`D07` → CPF1, `D10`–`D11` → COX0 (`-AC1`); formatter selection adds no new switch                                                                            |
| Untagged coordinate/public identity       | UAK0 `D08` → CPF1, `D09` → COX0, `D16` → ENCF0, `D17` → ENCL0, `D19` → PM1 (original receiving acceptances); formatter-specific conversions belong to FMT4P/FMT4L/FMT4F |
| Duplicate component information authority | UAK0 `D12`–`D15` → TIF1, `D18` → IDX0 (`-AC1`); formatting consumes syntax/trivia, never a parallel component-info projection                                           |

These references preserve the [authority inventory](authority-inventory.md),
not a second deletion assignment. The only newly inventoried displaced route is
the whitespace formatter. Private printers/views/renderers have empty current
formatter deletion populations. None conditionally acquires an unnamed prototype.

## Authority, identities and request basis

**FD01 — One authority.** One private `FormatterService` composes native printer
contributions. FMT3C owns service registration, selection, aggregation and cache.
The immutable catalog selects exactly one outer carrier printer and declares
embedded contributions by region role and exact carrier/dialect. No family-name
switch, first-registration winner, nearest-language fallback or dual-running
formatter is admitted. A framework profile may supply semantic attachment context;
it never substitutes for the syntax carrier.

**FD02 — Exact basis.** A request captures source unit/canonical source, source
revision/content, carrier and observed parser grammar/syntax identity, relevant
activated framework and project profiles, catalog rows/epochs, normalized config
with ordered provenance, formatter policy/version, and requested operation.
Observe external region source/map revisions too. Compare every observed basis
before publication and reuse. Unobserved semantic profiles and external type-provider
process identities do not invalidate pure formatting. Format configuration must
not invalidate an unchanged parser artifact.

| Named boundary       | Formatter relationship; inherited implementation owner                                                                 |
| -------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `CarrierProfileId`   | Syntax/dialect and outer/embedded printer selection; VID0T supplies the nominal identity                               |
| `FrameworkProfileId` | Activated attachment context only when observed; VID0T supplies identity                                               |
| `ProjectProfileId`   | Captured configuration/resource context when observed; VID0T supplies identity                                         |
| `CatalogSnapshot`    | Immutable printer/view/embedded contribution rows and epochs; CPF1 constructs the catalog                              |
| `DemandPlan`         | Explicit operation and selected/requested regions; COX0 supplies planning, FMT3C consumes demand                       |
| `TypeInfoRequest`    | Remains TIF1's semantic request; formatter performs zero TypeInfo queries or independent component-info reconstruction |

At this source, the first five names are future boundaries (see the authority
inventory); `TypeInfoRequest` aliases the graph request. Their absence does not
mean they are shipped or disprove the declared migration.

**FD03 — Demand and result truth.** Selection alone does no formatting work.
Disabled, inapplicable and unrequested operations perform zero parse, view, Doc,
render, edit and map work. Validate capability, option and input basis before
execution. An ignored file is an explicit skipped/inapplicable outcome, not a
formatted Complete result. NeedInputs, invalid configuration, unsupported,
cancelled, stale and partial outcomes preserve typed reasons and cannot become
successful empty edits or cache entries. A Complete unchanged result has exact
identity/map truth and zero edits; it differs from a refusal.

## Configuration vocabulary and compatibility cells

**FD04 — One translator.** CFG0 owns capture, precedence, trust and prepared
inputs. FCFG0 alone interprets the opaque `format` section and captured Prettier
options/config/ignore/override data into private `FormatterConfig`. Ordered origin
records retain file/layer, selector, option/value, and contributing snapshot.
A trusted host may capture executable config; the formatter never executes it.
Verter-only controls use a separate namespace. Renderers/printers/adapters consume
normalized options and perform no private discovery or normalization.

The vocabulary is divided into explicit families:

| Family                      | Prettier spellings to classify against FMT0's selected release                                                                                                                                    |
| --------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Layout                      | `printWidth`, `tabWidth`, `useTabs`, `endOfLine`                                                                                                                                                  |
| JS/TS/JSX tokens and groups | `semi`, `singleQuote`, `quoteProps`, `jsxSingleQuote`, `trailingComma`, `bracketSpacing`, `bracketSameLine`, `arrowParens`, `objectWrap`, `experimentalTernaries`, `experimentalOperatorPosition` |
| Carrier/embedded policy     | `htmlWhitespaceSensitivity`, `vueIndentScriptAndStyle`, `embeddedLanguageFormatting`, `singleAttributePerLine`, `proseWrap`                                                                       |
| Admission/pragma            | `requirePragma`, `insertPragma`, `checkIgnorePragma`                                                                                                                                              |
| Request/tooling controls    | `parser`, `filepath`, `rangeStart`, `rangeEnd`, `cursorOffset`, `plugins`, ignore and override inputs                                                                                             |

This family list is a classification vocabulary, not an admitted option matrix or
version pin. FMT0 selects a literal Prettier release and enumerates its complete
option names, types, defaults, finite values/bounds, applicability and unsupported
values; release-specific/deprecated spellings are explicit rows. Omitted names
never silently default. An integer width is validated against its declared bound;
`printWidth` is a layout target, not permission to break literals or recovery
bytes. Request geometry becomes checked authored ranges/cursors at the adapter;
`parser`/`filepath` cannot override the resolved carrier, and executable plugins
cannot delegate production formatting. Unknown, invalid, inapplicable and known
unsupported values have distinct typed diagnostics. Explicit inapplicable options
follow the locked cell policy, never silent dropping. Defaults apply only to
absent admitted options, with default provenance.

**FD05 — Cell schema.** FMT0's `schema = 1` compatibility catalog has:

- `option` rows: `name`, `value_kind`, `default_json`, `applicable_languages`,
  `unsupported_values`;
- `corpus` rows: `id`, hermetic repository-relative `path`, content `sha256`,
  exact `language`, `purpose`;
- `cell` rows: `id`, `language`, `corpus_id`, `option_set`, `recovery_mode`,
  `request_mode`, `classification`, `expected_outcome`.

Every admitted language × option-set × recovery × request cell occurs once.
A cell's corpus and option-set fix the source and configuration basis; carrier,
dialect and printer epochs qualify implementation identity. Request mode is full,
authored range or cursor-bearing only where the surface supports it. Recovery is
valid, exact retained islands, or typed unsupported as locked by that grammar.
`expected_outcome` is exact checked-in output/error data or a literal unsupported
reason; it never points at a live network oracle. A `verter-default` cell also
binds a pre-ratified divergence reason and expected bytes in its reviewed fixture
metadata. Changing a classifier or expected bytes after observing a mismatch
requires ratification; a new divergence cannot convert a regression into success.

**FD06 — Profiles.** `prettier-exact` requires byte-identical output to the pinned
Prettier oracle for that exact cell, including newline/pragma/recovery policy.
`verter-default` requires the exact independently locked native output and the
pre-ratified divergence; it makes no blanket Prettier parity claim. `unsupported`
returns its typed reason before printer work and never formatting success.
These labels are behavior classifications, not framework identities or fallback
order. An exact request cannot degrade to default or to whitespace-only output.
A mixed outer/embedded result claims exact parity only if its complete composition
cell, including boundary policy, is exact. One unsupported required child prevents
a Complete outer result; retained opaque text is allowed only by an explicit
source-backed verbatim policy in the admitted cell.

**FD07 — Oracles only.** Prettier supplies versioned compatibility evidence;
oxfmt supplies bug/counterexample evidence only. Neither is a runtime dependency,
printer delegate, alternative option vocabulary or config matrix. No client-side
external-tool fallback is part of the native authority.

## Authored views, Doc algebra and composition

**FD08 — Source-backed view.** The printer view borrows the existing parser's
accepted source artifact, tokens/trivia, block boundaries, raw text, delimiters and
recovery records. It must cover the entire authored carrier extent in stable
source order, including comments, whitespace, BOM, CRLF, holes, custom blocks and
EOF. It cannot reconstruct source from a lossy semantic AST or parse a second time.
Existing OXC/other AST data is usable only together with exact source/trivia and
recovery coverage. A grammar lacking that coverage stays unsupported until its
own view owner proves it. Formatting is never a parse side effect.

**FD09 — Provenance.** FMT1P owns private authored/formatted UTF-8 offset/range
types, `FormatProvenanceId`, `AuthoredProvenance`, `Provenanced<T>` and
`FormatProvenanceTable<R>`. Each ID denotes one authored unit in one source
revision/table and binds one exact range. Same-revision view reconstruction is
deterministic. Duplicate conflicting bindings, missing rendered IDs and mixed
revisions fail. Synthetic output is explicitly synthetic and cannot mint an
editable authored span. These are not wire identities and have no implicit
integer/domain conversions or generated-TSX coordinate reuse.

**FD10 — Document algebra.** FMT1 owns source-backed text, concatenation, groups,
nesting/indentation, conditional flat/broken documents, soft/hard lines and ordered
line suffixes. A deterministic width decision selects flat or broken layout;
width measurement, tabs, Unicode, line endings and suffix flush rules are fixed
by FMT0 fixtures. Literal/raw/recovery bytes never pass through a whitespace
normalizer. The renderer consumes only Doc plus normalized options, emits once
into one final sink, carries the FMT1P provenance on segments, and cannot parse,
resolve, invoke TypeInfo or dispatch a request. Traversal and group decisions
have explicit bounded work stacks, not unbounded recursive search.

**FD11 — Recovery islands.** FMT1A records malformed/unsupported syntax as exact
source-backed islands. Admission decides to retain an island whole or refuse;
retention preserves its bytes, range and provenance without fabricated syntax.
Boundary indentation/newlines may change only where the grammar's locked view
marks safe trivia outside the island. An island that prevents safe composition or
unique edit mapping produces typed partiality/unsupported, never guessed text.

**FD12 — Composition.** FMT3C calls the unique outer printer once, and each
requested embedded region once, in authored order. JS/TS/JSX/TSX, CSS/SCSS/Less,
HTML, Vue, Svelte and Pug contributions keep their separate grammar owners. Region
roles and exact catalog rows select them, not tag spellings. The outer printer
owns delimiters and boundary trivia; the child owns only its declared content.
Children return provenance-bearing Doc/view contributions for shared rendering,
not preformatted strings for format-after-build surgery. Decode/project/re-encode
only through EMB0I's declared codec and exact reversible map chain. Holes remain
separate expression regions; no guessed proportional mapping or private codec.

An external resource retains its own source unit, base URI and revision; a document
request cannot edit an external file as if it were an inline block. Such text is
unchanged or explicitly unsupported by its cell, unless a separately authorized
multi-resource operation exists. Coalesced contributions preserve source/region
order, cannot overlap ownership, and share the renderer, edit and map authorities.
Lint suppressions and rule fixes do not acquire formatter ownership. Formatter
suppression/pragma support requires explicit admitted policy; it cannot infer
semantic lint/action rules.

## Edits, position maps, range and cursor

**FD13 — Edits.** FMT1B derives the bounded, deterministic edit set. Edits are in
bounds at scalar boundaries, sorted, non-overlapping and contain no no-op
replacement. Applying them to the exact authored revision yields rendered bytes
exactly. Preserve unchanged prefix/suffix and internal stable regions according
to the locked minimality algorithm; avoid a whole-file replacement when a smaller
set is proven. Equal-boundary insertion order and newline policy are fixed by
fixtures. No unbounded quadratic diff fallback is permitted.

**FD14 — Maps.** FMT1C joins view and rendered provenance through FMT1P IDs.
Retained positions have exact correspondence; inserted/deleted/replaced regions
are explicit, not fabricated bijections. At a changed or shared boundary use the
locked left/right bias, with typed ambiguity/absence where no unique preimage
exists. Unicode scalars, CRLF, zero-width insertions, BOM and EOF are covered.
Recovery islands retain source-backed correspondence. EMB0I composes an embedded
map only if every endpoint's source/content/revision/domain equals the next.
Formatter position maps remain distinct from compiler/source maps and from
lint/action edit transactions; one map cannot certify another operation's safety.

**FD15 — Range.** FMT1D expands an authored selection to the smallest locked safe
format unit. Never split a token, raw-text/recovery island, hole or required
delimiter pair. Return the expanded range and sorted edits confined to it plus
explicitly locked boundary whitespace. Expansion requiring an unowned region or
non-unique codec preimage is refused. Already expanded range output agrees with
the corresponding full-format slice. Outside bytes remain unchanged.

**FD16 — Cursor.** FMT1E projects a cursor through the existing map/edit index,
with fixture-locked affinity at retained, inserted, removed and replaced regions.
A removed position may use a declared boundary anchor; it cannot invent a semantic
anchor. Reject invalid scalar/surrogate boundaries at admission; never clamp.
The result cursor indexes formatted output, not original or generated text.

FMT0 fills the exact algorithm/policy fixtures for FD10–FD16. Those mechanisms
remain with their implementation owners; this decision creates no alternate
renderer, differ, codec or coordinate owner.

## Public capabilities and conversion

Private support does not advertise a public capability. The planned cells are:

| Surface; owner            | Full document | Authored range | Cursor result | Boundary                                                                |
| ------------------------- | ------------- | -------------- | ------------- | ----------------------------------------------------------------------- |
| Private service; FMT3C    | yes           | yes            | yes           | Private authored/formatted UTF-8 domains                                |
| Rust protocol; FMT4P      | yes           | yes            | unavailable   | SFC-absolute checked `Span`, no public cursor field                     |
| LSP; FMT4L, cutover FMT3  | yes           | unavailable    | unavailable   | Standard `TextEdit`, negotiated `LineIndex` encoding                    |
| NAPI; FMT4N through FMT4F | yes           | yes            | yes           | Strict UTF-16 ranges/edits, authored input and formatted output cursors |
| WASM; FMT4W through FMT4F | yes           | yes            | yes           | Same FMT4F conversion, no fork                                          |
| MCP; FMT4M                | yes           | yes            | unavailable   | SFC-absolute checked `Span`, no cursor tool cell                        |

FMT4P validates source/revision/text and private domain conversion before one
service call. FMT4F is the sole cursor-bearing private adapter caller/result
consumer and strictly converts UTF-16 against the correct input/output snapshots.
FMT4L converts edits using the exact authored document and negotiated UTF-8/16/32.
No 0:0, nearest-token, wrong-source or default-encoding fallback is allowed.
Unsupported/partial/cancelled results cannot serialize as Complete success.
LSP range/cursor additions require separate ratified capability/route ownership;
on-type auto-close remains untouched. CLI and other future surfaces make no
support claim here; their own route node and promotion proof are required.

## Stability, incremental and performance gates

**FD17 — Stability.** For every admitted complete cell, let `F` be native
formatting with fixed config/policy: `F(F(source)) = F(source)` byte for byte.
The second request returns zero edits. Same basis under different discovery or
scheduling order yields identical output, ordered edits and map answers; provenance
assignment is deterministic within each exact source revision. Formatting must
preserve grammar semantics and significant literal/recovery bytes, not merely
produce parseable text. A `prettier-exact` comparison checks exact bytes without
sorting or whitespace normalization. Unaffected grammar/option fixtures remain
byte-identical when a contribution is added.

**FD18 — Incremental.** Same final source/catalog/config/policy basis gives the
same fresh and incremental complete output, edits and map answers. Edit/revert,
profile/config/policy transitions, cancellation and stale completion are explicit
cases. No stale, partial, unsupported, cancelled or mismatched result warms or
publishes; complete unaffected regions preserve identity. FMT3C owns the cache,
not views or public adapters. It admits at most two identities per open canonical
source, 64 entries and 96 MiB simultaneously, with deterministic LRU eviction and
no entries for closed/unsupported/disabled sources, as locked by FMT0.

**FD19 — Work and measurement.** Count semantic parser invocations (zero), view
visits, Doc creation/render/group decisions, final-sink emission, intermediate
copies, edit comparisons, map segments/query probes, stack growth and retained
complete result/cache work. FMT0 binds units to authored/input/output size,
carrier/view inventory, edits, segments and query counts; counters cannot omit
revisits or repeated copies. Exact warm, changed-region, edit/revert, transitions,
project open/close, churn, range/cursor, cancellation and disabled/unrequested
modes are separately evidenced. Provider transitions perform zero formatter work.
No second parse, private map/render fork or hidden eager prewarm is allowed.

FMT0 owns exact corpora, bounded-work formulas, the action manifest, gate wiring
and `formatter-performance` evidence-run registration. The standing measurement
rule governs conflicting numeric text: same-worker baseline/candidate answer
classes and work counts/growth ratios establish performance acceptance; timings,
allocation and memory measurements are advisory evidence. Unavailable controls
are recorded unavailable and do not fail or park this decision. Absolute time and
memory gates belong only to the performance phase's methodology. This page adds
no counter, benchmark, soak or CI lane.

## Evidence selection

| Acceptance | Smallest discriminating evidence and limit                                                                                                                                                                                                                                                |
| ---------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| FMK0-AC1   | Reviewed complete inventory/census, exact receiving clauses and controller successor edges. Executable omission/conflict rejection belongs to UAP0-AC-R1; actual deletion belongs to FMT3 and inherited route owners.                                                                     |
| FMK0-AC2   | Existing parse-key identity, diagnostic-order permutation and protocol request/outcome round-trip tests cited in the case table. They prove current invariants only; native formatter architecture fixtures remain UAP0 subblock 3 and printer/map behavior remains its named successors. |
| FMK0-AC3   | Runtime proof is not applicable to this docs-only diff: no cache, cancellation, publication or partial-result authority changes. FD18 specifies the receiving obligation for FMT3C-AC3 and FCFG0-AC3.                                                                                     |
| FMK0-AC4   | Runtime proof is not applicable: no hot-path code, allocation, parse, emit or retention changes. FD19 binds bounded-work ownership to FMT0 and implementation acceptances; no synthetic benchmark is added.                                                                               |

Targeted package tests are iteration evidence, not proof of future formatting or
independent acceptance. The workflow owns current-candidate review and the full
CI gate; no historical Git object, committed transcript or refreshed result count
is an acceptance input.
