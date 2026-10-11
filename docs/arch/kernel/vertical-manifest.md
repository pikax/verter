# Vertical conformance manifest schema

This decision fixes the one shape in which every architecture obligation of an
exact vertical release is written down once: its identities, geometry, parser
decision, activation, embeddings, maps, TypeInfo roles, Custom Element
dispositions, coexistence, public surfaces, budgets, compiler state, deletions,
forbidden dependencies, capabilities, rules, oracles and fixtures. Today,
framework-shaped host/session registries and untagged public boundaries own
these facts, spread over Rust enums, runtime tables and hand-maintained
evidence files. The final and sole owner is the typed immutable universal
catalog and the demand-selected kernel services; the manifest is the reviewed
data those services are built from.

It describes the repository at `docs(arch): record the parser decision,
ownership, reuse and lineage (#812)`, 2026-10-09. It follows the docs-only
rule in [README.md](README.md): it changes no production route and adds no
check. It builds on the [authority inventory](authority-inventory.md), the
[constitution](constitution.md), the [identities](identities.md), the
[catalog](catalog.md), the [demand plan](demand-activation.md) and the
[parser decision](parser-ownership.md). It does not re-own anything they
assign.

## Machine-readable products

The reviewed contract data lives in `tests/kernel/VIM0/products/`:

| File                          | Holds                                                                                                                                                                                                                                                                                                                                                                                              |
| ----------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `manifest-inventory.v1.json`  | Manifest files `MF01`–`MF05`, the subblock-6 sections `MS01`–`MS12`, schema rules `VM01`–`VM28`, cell tiers `CT0`–`CT4`, current manifest-like authorities `G01`–`G09`, outcomes `VIM-O..`, consumers `VIM-C..`, plan consumers `VIM-P..`, displaced routes `VIM-D01`–`VIM-D04`, referenced routes, category coverage, empty populations, findings `VIM-F..` and transferred obligations `VIM-T..` |
| `manifest-case-table.v1.json` | Positive manifests `MP01`–`MP07`, structural-failure cases `MN01`–`MN17` and work counters `WC01`–`WC02`, each with input, required and forbidden outcome, the rules it exercises and the node whose test makes it executable                                                                                                                                                                      |

A `successorPath` follows successor edges in the controller-owned plan.
Routes this node displaces start at VIM0. Routes an earlier decision already
assigned keep that decision's path: TIF1, IDX0 and CPF1 are not successors of
VIM0, so a path that started here would not be an edge. UAK0's `D..`/`O..`,
UAK1's `U..`, VID0's `I..`/`R..`/`V-..`, CAT0's `T..`/`CR..`/`CAT-..`, DEM0's
`DR..`/`DEM-..` and PAR0's `PD..`/`PAR-..` rows keep their owners. VIM0
references them by id and does not re-home them.

## Method

1. Found every place where the repository holds per-framework, per-version
   capability, support, oracle or fixture truth as data or as a closed
   per-framework table (`G01`–`G09`).
2. Read each predecessor decision for the rows that already name "the vertical
   manifest" or "VIM0/VIM1" as their home (UAK1 `U03`, `U07`; PAR0 `PD01`,
   `PAR-O01`; CAT0 `T01`–`T09`; DEM0 `DR06`–`DR14`).
3. Wrote one schema section per obligation, each a typed rendering of a row an
   earlier decision already owns. A section adds shape, never meaning.
4. Fixed the representative manifests the charter names (current Vue and
   Svelte; synthetic HTML, React, Lit, Angular and a project profile) and the
   structural failures, as cases whose executable owner is VIM1.
5. Bound every outcome and consumer this node introduces to one descendant of
   VIM0 with a receiving acceptance ID. Displaced routes whose production
   owner is not a descendant keep the owner, path and acceptance the authority
   inventory already records.

## What exists at the described head

There is no vertical manifest. The facts it will hold live in nine places:

- **A hand-maintained capability matrix** (`G01`).
  `packages/framework-conformance-harness/evidence/capability-matrix.tsv` has
  34 rows keyed by `cell_id`, with a display `framework` column, an untagged
  `compatibility_domain` string (`core@3.6.0-rc.3`, `svelte@5.56.8`), a
  disposition, a maturity, and `owner`/`acceptance_id` columns that name
  retired program blocks.
- **A Rust mirror of that matrix** (`G02`). `CapabilityCell` (34 variants,
  `VueParseLocal` … `SvelteOtherVersion`) and `CapabilityDisposition` in
  `crates/verter_compiler/src/compile_request/capability.rs` copy the TSV by
  hand. `capability_matrix_compile_request_coverage`
  (`every_tsv_row_has_a_registered_verification`,
  `cell_ids_match_the_committed_matrix`) keeps the two in step. The session
  maps compile requests onto cells in `host_resolve/compile_request_build.rs`.
- **Oracle pins** (`G03`). `oracles/{vue,svelte}/package.json`,
  `package-lock.json` and `closure.tsv`, produced by
  `generate-oracle-closures.mjs`, pin `vue` `3.6.0-rc.5` and `svelte`
  `5.56.10` with npm integrity digests.
- **Official-case seed ledgers** (`G04`). `vue-official-cases.tsv` and
  `svelte-official-cases.tsv`, extracted by
  `generate-official-case-manifests.mjs`, with
  `official_parse_manifest_guard` reading them.
- **Option inventories** (`G05`). `vue-options.tsv`, `svelte-options.tsv`.
- **A closed tag table** (`G06`). `FrameworkAdapterRegistry::tag_disposition`
  matches `FrameworkTag` by hand: Svelte `DeferredVertical` when absent,
  React and Solid `OutOfScope`. This is VID0 `V-D03` (REG0).
- **Generated client mirrors** (`G07`). The client framework manifest and the
  virtual-file naming mirror, byte-pinned by
  `client_framework_manifest_ts_is_byte_equal_to_the_rendered_registry` and
  `virtual_file_naming_ts_is_byte_equal_to_the_rendered_descriptor_column`.
  Their generators are CAT0 `CAT-D04` (CPF1).
- **Evidence-class policy** (`G08`). WDX0's `evidence-class-policy.v1.json`
  defines `static-proof`, `runtime-observation` and `estimate`, with the law
  that a static proof never satisfies a support claim.
- **Performance gates** (`G09`). `performance-gates.toml` holds the ratified
  gate rows.

`G03`–`G05` and `G08`–`G09` stay. `G03`–`G05` become inputs that a manifest
cites by path and digest; `G08` and `G09` are vocabularies a manifest
references by id. `G06` and `G07` keep their recorded owners. `G01` stops
being an authority (`VIM-D04`): VIM1 renders the matrix under `verticals/`.
FCH1 follows VIM1 and its charter path contains the harness TSV. `FCH1-AC3`
does not state the cutover. It joins coverage onto
`tests/framework-<f>/<P>0/products/*-capability-matrix.json` and can pass
while the TSV stays hand-maintained, so it is not the receiving acceptance.
`VIM-T05` records receiving acceptance `FCH1-AC5`, authorised by ruling
`fch1-matrix-cutover-acceptance` (2026-10-10): after `VIM1-AC-R2`, FCH1
renders that TSV from the canonical matrix or deletes it, so hand-maintained
disposition, maturity, owner and acceptance columns cannot remain a second
authority. Its proof must reject hand edits to a rendered TSV or show the TSV
deleted and all readers migrated. Existing Vue/Svelte goldens and dependency
pins remain intact. `FCH1-AC3` stays the coverage join and does not authorise
this cutover. VIM1 already precedes FCH1. Implementation remains FCH1's after
VIM1.
`G02` is deleted by NCK5 after that render's freshness guard holds (`VIM-D01`).

## Manifest layout

A carrier manifest is one directory keyed by `CarrierProfileId` alone. A
semantic-profile or project-profile manifest is one directory per exact
release:

```
verticals/<vertical>/                         # kind = carrier
verticals/<vertical>/<release>/               # kind = semantic-profile | project-profile
  vertical.toml            # MF01: identity, geometry, ownership, version + sections MS01–MS12
  capabilities.toml        # MF02: capability cells
  rules/<rule-pack>.toml   # MF03: rule and action rows
  oracles.lock             # MF04: exact oracle packages and corpora
  fixtures/manifest.toml   # MF05: fixture inventory
```

`<vertical>` is the catalog slug of the carrier, semantic profile or project
profile. `<release>` is present only for a profile kind, and it is that
profile's exact `ReleaseId` (VID0 `I05` for a semantic profile, `I07` for a
project profile). A carrier identity never contains a `ReleaseId` (VID0
`I03`), so a carrier directory has no release component and one
`CarrierProfileId` has one manifest. The directory release and the
`ReleaseId` inside the profile id are the same value; a second release for
the same profile id fails as CAT0 `CR07`. Paths are portable
(`tracked_paths_are_portable`): a release directory spells the version with
`.`, `-` and `+` only. The `verticals/` tree is VIM1's to create
(`VIM1-AC-R1`).

### `MF01` — `vertical.toml`

| Key              | Type                                                                                                                    | Rule   |
| ---------------- | ----------------------------------------------------------------------------------------------------------------------- | ------ |
| `schema`         | `ManifestSchemaEpoch` integer                                                                                           | `VM01` |
| `kind`           | one of `carrier`, `semantic-profile`, `project-profile`                                                                 | `VM04` |
| `id`             | the tagged `CarrierProfileId`, `FrameworkProfileId` or `ProjectProfileId` of `kind`                                     | `VM03` |
| `release`        | one exact `ReleaseId` scalar; semantic-profile and project-profile only. Forbidden on `kind = carrier`                  | `VM02` |
| `display_family` | presentation string, never a key                                                                                        | `VM03` |
| `geometry`       | carrier axis, claim shape, or `not-applicable` (below)                                                                  | `VM05` |
| `owner`          | the plan node owning the vertical and its terminal                                                                      | `VM06` |
| `claims`         | for a semantic profile, the `CarrierProfileId`s it claims; for a project profile, the `FrameworkProfileId`s it overlays | `VM07` |

`GeometryClass` is closed: `dedicated-carrier`, `script-carrier`,
`neutral-carrier`, `embedded-syntax`, `document-language`, `dsl`, `overlay`
(PAR0 `DK5` on JS/TS/JSX/TSX), `attachment` (PAR0 `DK5` on HTML attributes or
tagged templates) and `project-overlay`. Which kind may use which value is
`VM05`. A representative geometry that needs a value outside this list
rescopes the schema (`VM28`); it never adds an untyped escape.

### Subblock-6 sections of `vertical.toml`

Each section is a typed table that renders a row an earlier decision owns.
Every section is mandatory for the `kind`s the inventory lists; a section that
does not apply is written as `not-applicable` with a typed reason, never
omitted (`VM08`).

| Id     | Section                    | Renders                                                                                                                                                                                                                       | Rule   |
| ------ | -------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------ |
| `MS01` | `[parser]`                 | PAR0 `PD01` `ParserDecision`: grammar and exact edition, kind `DK1`–`DK5`, owner home or route, `ParserId`, `ParserGrammarEpoch`, lineage, license, oracle corpus, recovery and budget class, delegations, selecting evidence | `VM09` |
| `MS02` | `[activation]`             | DEM0 stage evidence: the parse-domain fact kinds stage one may read (`DR07`) and the post-snapshot fact kinds stage two may read (`DR11`); the FWA1 activation source                                                         | `VM10` |
| `MS03` | `[[embedding]]`            | CAT0 `T05`: host carrier, region role, embedded carrier, codec, map-chain kind                                                                                                                                                | `VM11` |
| `MS04` | `[[map]]`                  | the map-chain kinds `MS03` names, with ENC0 coordinate kinds at each end                                                                                                                                                      | `VM11` |
| `MS05` | `[[typeinfo_role]]`        | per public facet (props, events, slots, expose, and the profile's own), the producing construct and whether it is `native` or `derived{kind}`                                                                                 | `VM12` |
| `MS06` | `[custom_elements]`        | the CEF0 producer and consumer dispositions, one row each                                                                                                                                                                     | `VM13` |
| `MS07` | `[[coexistence]]`          | CAT0 `T06`: peer profile and `exclusive`, `coexisting` or `nested`                                                                                                                                                            | `VM14` |
| `MS08` | `[[public_surface]]`       | the surfaces (Rust, NAPI, WASM, LSP, MCP, CLI, VS Code, TS plugin) the vertical publishes on, with their PUB0 outcome vocabulary                                                                                              | `VM15` |
| `MS09` | `[[budget]]`               | references to `performance-gates.toml` rows, the MEM0 budget or the owning product catalog, by id                                                                                                                             | `VM16` |
| `MS10` | `[compiler]`               | CAT0 `T02`: `none`, or a backend for one referenced `FrameworkProfileId` (the release is that id's `ReleaseId`, not a key on the carrier)                                                                                     | `VM17` |
| `MS11` | `[[deletion]]`             | displaced routes the vertical's nodes delete: route id, owner node, receiving acceptance                                                                                                                                      | `VM18` |
| `MS12` | `[[forbidden_dependency]]` | UAK1 firewall edges the vertical's crates must not take                                                                                                                                                                       | `VM19` |

### `MF02` — `capabilities.toml`

One `[[cell]]` per capability. The cell key is the profile-qualified
`CapabilityId` (CAT0 `T07`, VID0 `I10`) and nothing else. That id already
carries its `FrameworkProfileId` or `CarrierProfileId`, and a
`FrameworkProfileId` already carries its one `ReleaseId` (VID0 `I05`), so the
cell does not restate profile, carrier or release. `host` is not a VID0
identity and is not a catalog key (VID0 `R14`); an extension is a CAT0 `T09`
row, not part of the `T07` key. Per-surface maturity is a value: one entry
per `MS08` surface, each with a tier, a producer, a receiving acceptance and
evidence ids. A required capability sets `required = true` on the cell.

Cell tiers (`CT..`) separate proof, partial implementation and delivered
product:

| Tier                           | Meaning                                                                                                             | Evidence class (WDX0)                                                                                                                   | May display as            |
| ------------------------------ | ------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------- | ------------------------- |
| `CT0` `excluded`               | a truthful exclusion with a typed reason and, for a migration exclusion, its target                                 | none                                                                                                                                    | excluded, with reason     |
| `CT1` `required-unimplemented` | a required cell no producer has delivered                                                                           | none                                                                                                                                    | not supported             |
| `CT2` `geometry-proof`         | the carrier geometry, identity or parser decision is proven                                                         | `static-proof`                                                                                                                          | proof only; never support |
| `CT3` `implemented-operation`  | the operation runs through its public boundary in the repository's executed tests                                   | none. WDX0 `static-proof` may not claim executed behaviour, and `runtime-observation` is the `CT4` receipt. The evidence is the test id | implemented, unqualified  |
| `CT4` `qualified-product`      | the vertical's own producing terminal observed the operation on the named surface and profile under a receipt basis | `runtime-observation`                                                                                                                   | supported                 |

The schema version declares, per `kind` and geometry, the normative required
capability list. Every required capability has one cell, and that cell's
maturity value names every surface the manifest's `MS08` publishes (`VM21`).
"Full support" of a release is derived: every required cell is `CT4` on every
published surface. It is never authored. Host identity is not a key.

### `MF03` — `rules/*.toml`

One file per rule pack. Each `[[rule]]` renders a CAT0 `T08` row: rule id,
profile, rule epoch, applicability, fix and action kinds. LRA0 owns the
vocabulary; the manifest only lists rows (`VM22`).

### `MF04` — `oracles.lock`

One `[[oracle]]` per upstream package or corpus the vertical tests against:
name, exact released version, registry or source, integrity digest, role
(`oracle`, never `dependency`) and the checked-in path of any corpus with its
content digest (PAR0 `PD04`). The manifest's `release` must equal the pinned
version of the oracle that defines it (`VM23`). `G03` is the seed.

### `MF05` — `fixtures/manifest.toml`

One `[[fixture]]` per fixture: id, repository-relative path, content digest,
class (`positive`, `negative`, `recovery`, `migration`), the cells or rules it
exercises and the acceptance ID whose test runs it (`VM24`). `G04` is the seed
for the Vue and Svelte official cases.

## Schema rules

### Identity and version

- **VM01.** Every file carries `schema = <ManifestSchemaEpoch>`. A schema
  change mints a new epoch; there is no unversioned manifest.
- **VM02.** A semantic-profile or project-profile manifest declares exactly
  one release (VID0 `R04`). `release` is a scalar exact version and equals the
  `ReleaseId` inside that profile id (`I05`, `I07`) and the directory name.
  A carrier manifest declares none (VID0 `R04`): `CarrierProfileId` never
  contains a `ReleaseId` (`I03`) and stays equal when the release changes
  (`R02`), so a carrier directory has no release component. A `release` key
  on a carrier, a `versions` array, a range, a `latest`/`next` tag, a
  wildcard or a complement ("anything other than") is a structural failure
  (`MN01`). An unsupported version is DEM0's typed `UnsupportedVersion`
  outcome, never a capability cell. One carrier id has one manifest; two
  releases for one profile id fail as CAT0 `CR07`.
- **VM03.** Every key is a typed identity in its canonical tagged encoding
  (VID0 `R12`, `R13`). `display_family`, adapter spellings, `FrameworkTag`s
  and file extensions are presentation data and never keys (VID0 `R03`, CAT0
  `CR10`). A cell does not restate the profile, carrier or release its
  `CapabilityId` already carries.
- **VM04.** `kind` decides which identity `id` is. A project-profile manifest
  declares no carrier, parser or compiler; a carrier manifest declares no
  semantic facet and no `ReleaseId`.
- **VM05.** Geometry of a carrier is an axis of `CarrierProfileId` (VID0
  `I03`, identities.md). On `kind = carrier`, `geometry` is one of
  `dedicated-carrier`, `script-carrier`, `neutral-carrier`,
  `embedded-syntax`, `document-language`, `dsl`, and it is the only geometry
  authority for that carrier. A semantic profile does not restate that axis.
  Its `geometry` is the claim shape `overlay` or `attachment`, or
  `not-applicable` with reason `carrier-owned` when the geometry lives only
  on the claimed carrier (Vue, Svelte, the Angular carrier). A project
  profile's `geometry` is `project-overlay` only: `ProjectProfileId`
  references semantic profiles and carries no carrier geometry (VID0 `I07`).
  A profile `geometry` equal to a claimed carrier's geometry, a carrier
  geometry class on a profile, or `overlay` / `attachment` /
  `project-overlay` on a carrier, fails (`MN17`). The claimed carriers'
  geometries are read from those carrier manifests.
- **VM06.** `owner` and every producer, deletion and acceptance reference name
  an existing plan node and acceptance ID. Prose, retired block names and
  people are not owners.
- **VM07.** `claims` references existing manifests by typed id. A dangling
  claim fails like CAT0 `CR07`.

### Sections

- **VM08.** Closed schema: every table and key is declared by the schema
  epoch. An unknown key, an `extra`/`ext`/`x-*`/`metadata` bag, an untyped map
  or an inline free-form table is a structural failure (`MN02`). The only
  extension point is a typed `T09` contribution row (CAT0 `CR04`).
- **VM09.** `[parser]` and `[compiler]` are separate sections with no shared
  key. A parser entry that names a backend or compile capability, a compiler
  entry that names a grammar or `ParserId`, or a compile capability inferred
  from the presence of a parser is compiler/parser conflation (`MN04`; UAK1
  `U03`). A carrier with `DK5` has `[parser] kind = "no-parser"` and
  registers no frontend.
- **VM10.** `[activation]` stage-one fact kinds are parse-domain only. A
  stage-one entry that names TypeScript, the shared resolver, TypeInfo or any
  semantic oracle fails (`DR07`). The activation source is FWA1's record; a
  second source fails (`MN13`).
- **VM11.** Every embedding names a map-chain kind that `[[map]]` defines, and
  every map names ENC0 coordinate kinds at both ends. An embedding without a
  map, or a map with an untagged coordinate, fails.
- **VM12.** Every public facet the profile publishes has one `typeinfo_role`
  row. A `derived` row names its derivation kind.
- **VM13.** `[custom_elements]` always has exactly one producer row and one
  consumer row. Each is a CEF0 disposition, or `not-applicable` with a typed
  reason. A missing row is a structural failure, never read as "no custom
  elements" (`MN03`).
- **VM14.** Every peer that can claim one of this manifest's carriers has a
  `[[coexistence]]` row, or the overlap is a typed ambiguity (CAT0 `CR14`,
  DEM0 `DR22`). Two relations for one peer fail (`CR07`).
- **VM15.** Every surface listed in `[[public_surface]]` uses the PUB0 outcome
  vocabulary. A surface with a registered handler and no implemented cell is a
  boolean capability lie (UAK1 `U07`) and fails.
- **VM16.** A budget is a reference to a ratified row by id. A manifest that
  declares its own threshold, picks its own gate or omits a gate a required
  cell needs is a self-selected performance gate (`MN14`).
- **VM17.** `[compiler] = "none"` is a normal state (UAK1 `U02`). A backend
  on a carrier manifest names the `FrameworkProfileId` it serves. The release
  is that id's `ReleaseId`. The carrier manifest does not gain a `release`
  key from this section.
- **VM18.** Every `[[deletion]]` names a route id an earlier decision
  recorded, one owner node and its receiving acceptance.
- **VM19.** `[[forbidden_dependency]]` restates UAK1 firewall edges for the
  vertical's crates; the firewall itself stays UAM0's.

### Capabilities, evidence and support

- **VM20.** A capability cell is data, never prose. It names its tier, its
  producer node and receiving acceptance, and its evidence by test or receipt
  id. A row whose only content is a note is a prose-only capability row
  (`MN05`). Notes never carry meaning.
- **VM21.** The required capability list is normative per schema epoch, `kind`
  and geometry. There is one cell per required profile-qualified
  `CapabilityId` (CAT0 `T07`). That cell's per-surface maturity value names
  every surface `MS08` publishes, each with one tier. A manifest cannot drop a
  required cell, omit a published surface from the maturity value, mark a
  surface `excluded` without an exclusion reason the schema admits, or raise
  "full support" while any required maturity is below `CT4` (`MN09`, `MN10`).
  Two maturity entries for one surface fail. A `T09` extension is cited by row
  id from the value; it is not part of the cell key. Host identity is not a
  key (VID0 `R14`).
- **VM22.** `rules/*.toml` rows are CAT0 `T08` rows; their vocabulary is
  LRA0's.
- **VM23.** `oracles.lock` pins are exact and carry integrity digests. On a
  semantic-profile or project-profile manifest, `release` and the oracle pin
  that defines it agree; any divergence fails (`MN15`). A carrier manifest
  has no `release` to compare. A cell cites no separate compatibility
  release: the release is the one inside the profile id. An oracle is never a
  dependency and never an authority (PAR0 `PD04`).
- **VM24.** Every fixture has a content digest and an executing acceptance.
- **VM25.** Tier promotion follows evidence. `CT2` needs static proof
  (WDX0 `static-proof`). `CT3` needs executed tests through the public
  boundary, named by test id. `CT3` has no WDX0 evidence class: `static-proof`
  may not claim executed behaviour, and `runtime-observation` is reserved for
  `CT4`, a receipt from the vertical's own producing terminal (WDX0 claim
  laws). A geometry proof, an installed parser, syntax highlighting or an
  upstream install never promotes a maturity above `CT2` (`MN08`).

### Generation, determinism and project profiles

- **VM26.** Manifests are the authority for the facts a vertical declares.
  Rendering them does not transfer implementation ownership of the catalog
  rows. `T01`–`T03` stay CPF1 (`CPF1-AC2`), `T04` stays PPR0T (`PPR0T-AC1`),
  `T05` stays EMB0I (`EMB0I-AC1`), `T06` and `T07` stay COX0 (`COX0-AC2`),
  `T08` stays LNT3 (`LNT3-AC2`), `T09` stays XSDK1 (`XSDK1-AC1`), and `PD01`
  stays PAR0 (catalog.md, parser-ownership.md). VIM1 renders manifest bytes
  into those rows; the row owner remains the sole producer of the row. The
  capability matrix, the generated TS mirrors, docs support tables, the
  compile capability table and any per-family capability matrix are rendered
  consumers. A rendered artifact edited by hand fails its freshness check; a
  second hand-maintained truth beside a manifest fails (`MN07`).
- **VM27.** Manifests are data. They hold no script, expression, regex,
  template, code path, hook or plugin reference, and loading them executes
  nothing (`MN06`, `MN12`). Every table is ordered by the canonical encoding
  of its key; file and provider order never decide. Two clean renders are
  byte-identical.
- **VM28.** A project profile is keyed by one `ProjectProfileId` per exact
  release. A tool with incompatible majors, such as Tailwind v3 and v4, has one
  manifest per major. A migration manifest names a `source` and a `target`
  `ProjectProfileId` and never mints a merged identity. A workspace activates
  at most one of them per scope; both at once is DEM0's typed ambiguity
  (`MN11`).

The rule list is closed. A representative geometry that cannot be written
under these rules rescopes the schema through an amendment to this decision;
it never adds an untyped escape.

## Representative manifests

`manifest-case-table.v1.json` fixes the manifests the schema must accept.
Each is written in full there; these are their distinguishing values:

| Case                                          | `kind`                     | `geometry`                                                                                | `[parser]`                       | `[compiler]`                                          | `[custom_elements]`          |
| --------------------------------------------- | -------------------------- | ----------------------------------------------------------------------------------------- | -------------------------------- | ----------------------------------------------------- | ---------------------------- |
| `MP01` Vue 3.6 (current)                      | carrier + semantic-profile | carrier `dedicated-carrier`; profile `not-applicable` / `carrier-owned`                   | `DK4` existing, PAR0 `G03` route | backend: VDOM, Vapor, SSR, named `FrameworkProfileId` | producer and consumer        |
| `MP02` Svelte 5 (current)                     | carrier + semantic-profile | carrier `dedicated-carrier`; profile `not-applicable` / `carrier-owned`                   | `DK4` existing, PAR0 `G04` route | backend: client, server, named `FrameworkProfileId`   | producer and consumer        |
| `MP03` synthetic HTML                         | carrier                    | neutral-carrier; no `release` key                                                         | `DK3` fork into PAR0 `H08`       | `none`                                                | consumer only                |
| `MP04` synthetic React                        | semantic-profile           | claim shape `overlay` (the TSX carrier's geometry stays `script-carrier` on that carrier) | `no-parser` over TSX             | `none`                                                | consumer only                |
| `MP05` synthetic Lit                          | semantic-profile           | claim shape `attachment`                                                                  | `no-parser` on tagged templates  | `none`                                                | producer and consumer        |
| `MP06` synthetic Angular                      | carrier + semantic-profile | carrier `dedicated-carrier`; profile `not-applicable` / `carrier-owned`                   | `DK3` over `H08` or `DK4`        | `none`                                                | producer and consumer        |
| `MP07` synthetic project profile (Tailwind 4) | project-profile            | `project-overlay` only                                                                    | absent by `VM04`                 | absent by `VM04`                                      | `not-applicable` with reason |

A carrier and the semantic profile on it are two manifests (`VM04`); the
`MP01`, `MP02` and `MP06` rows are each such a pair. The synthetic cases are
schema fixtures, not support claims: their cells stay at `CT0`–`CT2`.

The structural failures `MN01`–`MN17` each name the one rule they break.
The charter's four named failures are `MN01` (versions array), `MN02`
(untyped extension bag), `MN03` (missing CE row) and `MN04`
(compiler/parser conflation). The web-product amendment adds `MN08`
(geometry proof shown as support), `MN09` and `MN10` (required cell dropped
or full support declared over one) and `MN11` (merged Tailwind identity).

## Displaced and referenced routes

| Route     | Category                                                                    | Unit                                                                                                                                                         | Disposition                                                                                                                                                                                                                                                                                                                                                                                                                    | Owner                                                                                                                                 |
| --------- | --------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------- |
| `VIM-D01` | duplicate capability authority; central framework switch; untagged identity | `CapabilityCell` / `CapabilityDisposition` (`G02`) in `crates/verter_compiler/src/compile_request/capability.rs`, a closed per-framework enum                | replace: the enum is deleted after VIM1's rendered-matrix freshness guard holds (`VIM1-AC-R2`). NCK5 is a successor of VIM1, rust-mixed, and its charter path `crates/verter_compiler` and domain `area:crates/verter_compiler` contain this file. CPF1 precedes VIM1, so CPF1 cannot be the deleter                                                                                                                           | NCK5 (`NCK5-AC1`), path `VIM0` → `VIM1` → `NCK5`                                                                                      |
| `VIM-D02` | duplicate component information                                             | UAK0 `D12`–`D15`: component-meta resolver/cache/schema, public-API projection, off-store surface caches, legacy serde TypeInfo DTOs (authority-inventory.md) | keep that assignment. A manifest declares facets and roles; it computes no component information. TIF1 is not a successor of VIM0                                                                                                                                                                                                                                                                                              | TIF1 (`TIF1-AC1`), path `UAK0` → `UAK1` → `CAT0` → `TIF1`                                                                             |
| `VIM-D03` | duplicate component information                                             | UAK0 `D18`: per-request component scan                                                                                                                       | keep that assignment. IDX0 is not a successor of VIM0                                                                                                                                                                                                                                                                                                                                                                          | IDX0 (`IDX0-AC1`), path `UAK0` → `UAK1` → `CAT0` → `DEM0` → `IDX0`                                                                    |
| `VIM-D04` | duplicate capability authority                                              | hand-maintained `capability-matrix.tsv` (`G01`)                                                                                                              | stops being an authority (`VM26`, `MN07`). VIM1 renders the canonical matrix under `verticals/` (`VIM1-AC-R2`). `FCH1-AC5` requires rendering the harness TSV from that matrix or deleting it. The TSV is not a G03–G05 seed. FCH1's charter path and domain contain the TSV; VIM1's do not. VIM1 already precedes FCH1. `FCH1-AC3` remains a coverage join | FCH1 (`FCH1-AC5`, ruling `fch1-matrix-cutover-acceptance` 2026-10-10), path `VIM0` → `VIM1` → `FCH1`; `VIM-T05` binds that acceptance |

`G06` (`V-D03`, REG0) and `G07` (`CAT-D04`, CPF1) stay with their owners.
`VueOtherVersion` and `SvelteOtherVersion` are complement arms of `G02`.
NCK5 deletes the arms with the enum (`VIM-F02`, `NCK5-AC1`). VIM1 proves a
complement is a structural manifest failure (`MN01`, `VIM1-AC-R1`), not a cell.

| Category                                  | Recorded here                                                        | Referenced                                         |
| ----------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------- |
| central framework switch                  | `VIM-D01` (closed per-framework `CapabilityCell`)                    | `D01`, `D04`, `D07`, `CAT-D06`, `V-D03`, `DEM-D04` |
| untagged coordinate/public identity       | `VIM-D01` (untagged `compatibility_domain`, display `framework` key) | `D08`, `V-D01`, `V-D02`, `PAR-D03`                 |
| duplicate component information authority | `VIM-D02` (`D12`–`D15`), `VIM-D03` (`D18`)                           | none left bare                                     |
| duplicate capability authority            | `VIM-D01`, `VIM-D04`                                                 | `CAT-D04`                                          |

## Outcomes and consumers

| Id        | Outcome or consumer                                                                                                                                                           | Owner                         |
| --------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------- |
| `VIM-O01` | Manifest schema types and structural validator for `MF01`–`MF05` and `MS01`–`MS12`; `MP01`–`MP07` accepted, `MN01`–`MN17` rejected for their own reason                       | VIM1 (`VIM1-AC-R1`)           |
| `VIM-O02` | Deterministic rendering of manifest sections into catalog rows `T01`–`T09` and PAR0 `PD01` decisions. Row owners stay CPF1, PPR0T, EMB0I, COX0, LNT3, XSDK1 and PAR0 (`VM26`) | VIM1 (`VIM1-AC2`)             |
| `VIM-O03` | Generated per-surface capability/maturity matrix from `MF02`, freshness-checked                                                                                               | VIM1 (`VIM1-AC-R2`)           |
| `VIM-O04` | Executable validator for this inventory: missing member, unknown or pathless owner, missing acceptance, conflicting assignment                                                | VIM1 (`VIM1-AC-R1`)           |
| `VIM-O05` | Independent re-validation of schema, generator determinism and malformed-manifest negatives                                                                                   | UAM0 (`UAM0-AC2`)             |
| `VIM-C01` | Compile-request construction consulting the rendered capability table (`G02`'s replacement)                                                                                   | NCK5 (`NCK5-AC1`)             |
| `VIM-C02` | Session compile-request mapping onto cells (`compile_request_build.rs`)                                                                                                       | VIM1 (`VIM1-AC2`)             |
| `VIM-C03` | Harness seeds `G03`–`G05` read into `oracles.lock` and `fixtures/manifest.toml`                                                                                               | VIM1 (`VIM1-AC-R1`)           |
| `VIM-C04` | Docs framework/version/carrier/surface support tables rendered from manifests; a `CT2` cell never shown as support                                                            | DOC6 (`DOC6-AC1`, `DOC6-AC2`) |
| `VIM-C05` | Language-service operation rows added to the manifest generator                                                                                                               | LSO9 (`LSO9-AC2`)             |
| `VIM-C06` | Diagnostic-family manifest built on the same schema                                                                                                                           | NCK4 (`NCK4-AC2`)             |
| `VIM-C07` | Enterprise conformance manifest and applicability closure                                                                                                                     | EPR6 (`EPR6-AC2`)             |
| `VIM-C08` | Utility-CSS project-profile cells, one profile per Tailwind major                                                                                                             | TW9 (`TW9-AC4`)               |
| `VIM-C09` | Skill planning and implementation routes reading the vertical manifest                                                                                                        | SKL0 (`SKL0-AC2`)             |
| `VIM-C10` | Cross-family convergence over every manifest                                                                                                                                  | UAK2 (`UAK2-AC1`)             |

The plan consumers `VIM-P01`–`VIM-P03` list which later successor reads which
rows, each with its own successor path and receiving acceptance.

## Findings recorded for the receiving owners

- **The matrix and the oracles disagree on the release** (`VIM-F01`, FCH1).
  `capability-matrix.tsv` says `core@3.6.0-rc.3` and `svelte@5.56.8`; the
  oracle `package.json` files pin `vue` `3.6.0-rc.5` and `svelte` `5.56.10`.
  Two hand-maintained truths have drifted. `VM23` makes the oracle pin the
  release and fails the divergence (`MN15`). FCH1 owns the TSV under
  `FCH1-AC5` (ruling `fch1-matrix-cutover-acceptance`, 2026-10-10);
  `FCH1-AC3` does not state that cutover. VIM1 cannot write that path.
- **Versions are encoded as capability cells** (`VIM-F02`, NCK5).
  `VueOtherVersion` and `SvelteOtherVersion` (domain "anything other than …")
  write a version complement as a cell. Under `VM02` that is DEM0's
  `UnsupportedVersion` activation outcome. NCK5 deletes the arms with the
  enum; VIM1 rejects the complement as `MN01`.
- **Matrix owners are not plan nodes** (`VIM-F03`, VIM1). The `owner` and
  `acceptance_id` columns name retired blocks (`B2`, `BV1`, `FC-…`). Migrating
  a row into `MF02` rebinds it to a plan node and acceptance ID (`VM06`).
- **VIM1 cannot delete `CapabilityCell`** (`VIM-F04`, NCK5). VIM1's domains
  are `area:verticals`, `area:xtask`, `shared:ci-workflow` and
  `area:.github`, and CPF1 precedes VIM1. `VIM-D01` is NCK5's: NCK5 follows
  VIM1 and its charter path `crates/verter_compiler` contains the file. The
  hand matrix is rendered under `verticals/` and the harness TSV is FCH1's
  (`VIM-D04`, `VIM-T05`).
- **The tag table calls React and Solid out of scope** (`VIM-F05`, REG0). The
  plan now carries React and Solid verticals. `V-D03` retags the table; a
  manifest's existence, not a hand row, decides a tag's disposition.
- **Family P0 locks plan their own capability matrices** (`VIM-F06`, VIM1).
  The family delivery locks (for example ANG0's
  `products/angular-capability-matrix.json`) each plan a family-local matrix.
  Under `VM26` such a matrix seeds that family's first manifest and is then a
  rendered consumer, never a live co-authority.

## Acceptance evidence

This change adds contract text and data only, so existing coverage and bounded
inspection are the right evidence. The diff adds no test, validator or check.

- **AC1 — ownership contract.** The inventory binds every manifest file,
  section, outcome, consumer and displaced route to one existing plan node,
  a successor path that exists in the controller plan, and a receiving
  acceptance ID. `VIM-D01` is NCK5 (`NCK5-AC1`) on `VIM0` → `VIM1` → `NCK5`,
  after the freshness guard, because that node's charter path contains
  `crates/verter_compiler/src/compile_request/capability.rs`. `VIM-D02` and
  `VIM-D03` keep TIF1 and IDX0, the owners the authority inventory assigns,
  on the UAK0 paths. `VIM-D04` binds FCH1 (`FCH1-AC5`) on `VIM0` → `VIM1` →
  `FCH1`: VIM1 renders under `verticals/`, and FCH1 owns the harness TSV.
  Ruling `fch1-matrix-cutover-acceptance` (2026-10-10) authorises `FCH1-AC5`.
  `VIM-T05` binds that acceptance. `FCH1-AC3` remains the coverage join and
  does not carry the cutover. Implementation stays with FCH1 after VIM1.
  UAK0, UAK1, VID0, CAT0, DEM0 and PAR0 rows are referenced, not re-owned.
  The executable validator for this inventory and the manifest negatives
  belongs to VIM1 (`VIM1-AC-R1`); UAM0 re-validates it (`UAM0-AC2`).
- **AC2 — positive contract.** Existing coverage pins the identity, provenance
  and ordering of the boundaries a manifest renders:
  - `capability_matrix_compile_request_coverage`
    (`every_tsv_row_has_a_registered_verification`,
    `cell_ids_match_the_committed_matrix`) for today's capability table;
  - `client_framework_manifest_ts_is_byte_equal_to_the_rendered_registry` and
    `virtual_file_naming_ts_is_byte_equal_to_the_rendered_descriptor_column`
    for rendered mirrors (`VM26`);
  - `framework_registry_complete` in `verter_session` `framework::registry`
    for tag dispositions;
  - `parse_key_canonical_bytes_and_digest_are_pinned` in
    `crates/verter_language/tests/cases/parse_identity.rs` and
    `every_canonical_grammar_input_discriminates_the_fingerprint` in
    `verter_language` `carrier_grammar` for `MS01` identities;
  - `typeinfo_proto_roundtrip` and `typeinfo_proto_ts_contract` in
    `crates/verter_protocol/tests/cases/` for `TypeInfoRequest`.

  New and extended tests belong to VIM1 (`VIM1-AC-R1`, `VIM1-AC2`).

- **AC3 — incremental equivalence: not applicable.** No cache, cancellation,
  stale-publication or partial-result authority is touched, and no production
  byte changes. A manifest is build-time data rendered into the snapshot,
  which is built once per host (CAT0 `CR01`); `VM27` binds render determinism
  to VIM1.
- **AC4 — bounded work: not applicable.** No hot path changes. `WC01` (two
  clean renders byte-identical) and `WC02` (rendering executes no vertical
  code) are VIM1's to prove.
