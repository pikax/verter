# Immutable typed catalog snapshot and static registration

This decision converges the registration roots that exist today into one
immutable, typed `CatalogSnapshot`. The snapshot has no flat row enum spanning
its tables, and no second registry stands beside it. Today,
framework-shaped host/session registries and untagged public boundaries own
registration. The final and sole owner is the typed immutable universal
catalog and the demand-selected kernel services.

It describes the repository at `docs(arch): ratify orthogonal identities and
the exact-release law (#802)`, 2026-10-09. It follows the docs-only rule in
[README.md](README.md): it changes no production route and adds no check. It
builds on the [authority inventory](authority-inventory.md), the
[constitution](constitution.md) and the [identities](identities.md) decision,
and does not re-own anything they assign.

## Machine-readable products

The reviewed contract data lives in `tests/kernel/CAT0/products/`:

| File | Holds |
| ---- | ----- |
| `catalog-inventory.v1.json` | Tables `T01`–`T09`, catalog rules `CR01`–`CR14`, contribution kinds `XK01`–`XK09`, outcomes `CAT-O01`–`CAT-O12`, consumers `CAT-C01`–`CAT-C06`, displaced routes `CAT-D01`–`CAT-D06`, the per-surface reference map, category coverage, out-of-scope registries, empty populations, findings and transferred obligations |
| `registration-fixture-matrix.v1.json` | Registration/capability cases `RM01`–`RM12`, determinism cases `DM01`–`DM05` and manifest byte-pinning cases `MB01`–`MB03`, each with input, required and forbidden outcome, existing evidence and the node whose test makes it executable |
| `ownership-negative-fixtures.v1.json` | The positive control `CN00` and the six refusal fixtures `CN01`–`CN06` for the ownership validator |

Every `successorPath` starts at CAT0 and follows predecessor edges in the
controller-owned plan. UAK0's `B..`, `O..`, `C..`, `S..` and `D..` rows,
UAK1's `U..` and `K..` rows and VID0's `I..`, `R..`, `V-..` rows keep their
owners; CAT0 references them by id.

## Current registration roots

Six roots register frameworks and carriers at the described head. They are
composed at host construction and kept consistent by runtime assertions:

| Root | Holds | Crate |
| ---- | ----- | ----- |
| `LanguageRegistry::built_in` / `global` | extension table: carriers, adapter modules, script rows | `verter_language` |
| `CarrierGrammarAuthority` | live carrier grammars, filled by `FrameworkCapabilityCatalog::register_all` | `verter_language` |
| `ImmutableCapabilityCatalog` + `built_in_*_catalog` | frontend, semantic, projection, host-integration and runtime rows | `verter_compiler` |
| `FrameworkCapabilityCatalog` | per-framework grammar rows, filtered by admission | `verter_session` |
| `FrameworkAdapterRegistry` + `built_in_descriptors` | adapter descriptors, registration legs, tag dispositions, provider-gate index | `verter_session` |
| two generated TS mirrors | client manifest and virtual-file naming, rendered from a descriptor-plus-extension-table join | `packages/language-shared` |

`HostServices::composed` builds the capability catalog and the adapter
registry from the same `FrameworkOptions`, then asserts in both directions
that they name the same frameworks, and `assert_classifier_agrees` checks the
classifier against them. Those checks exist only because the roots are
separate authorities.

The lint `RuleRegistry` is a seventh, mutable root: rules are pushed after
construction and filtered by `FrameworkAdapterId`.

## Snapshot shape

The snapshot is a set of separately typed tables. Each table has its own row
type and key, and each has one implementation owner:

| Table | Row key | Holds | Implementation owner |
| ----- | ------- | ----- | -------------------- |
| `T01` carrier | `CarrierProfileId` | extension claims, editor language ids, owner-local grammar config, frontend, role, syntax-schema epoch | CPF1 (`CPF1-AC2`) |
| `T02` compiler backend | `(CarrierProfileId, FrameworkProfileId)` | optional runtime backend and declared compile capability of that exact release | CPF1 (`CPF1-AC2`) |
| `T03` semantic profile | `FrameworkProfileId` | one `ReleaseId`, claimed carriers, display family, surface kinds, virtual-file naming, script-fact gates, synth and projector legs, activation slot, presentation rank | CPF1 (`CPF1-AC2`) |
| `T04` project profile | `ProjectProfileId` | overlaid semantic profiles, roles, realms, generated-fact kinds, applicability | PPR0T (`PPR0T-AC1`) |
| `T05` embedded language | host carrier + region role | embedded carrier, codec, map-chain kind | EMB0I (`EMB0I-AC1`) |
| `T06` interoperability | carrier + ordered profile pair | exclusive, coexisting or nested claim; narrowing kind | COX0 (`COX0-AC2`) |
| `T07` capability | profile-qualified `CapabilityId` | capability key, owner, per-surface maturity | COX0 (`COX0-AC2`) |
| `T08` rule and action | rule id + profile + rule epoch | applicability, fix/action kinds | LNT3 (`LNT3-AC2`; contract LRA0) |
| `T09` external contribution | kind + namespace + exact version | provenance, content digest, targets | XSDK1 (`XSDK1-AC1`; contract XSDK0) |

Plain script carriers (`ts`, `tsx`, `js`, `d.ts` and their module variants)
are `T01` rows too. A carrier row names no profile, so one byte stream can
carry several semantic claims (VID0 `VC03`). `T04`, `T06` and `T09` have no
rows at this head; each is an empty population recorded in the inventory.
The external-contribution consumer `CAT-C05` is cited from that `T09` empty
population, not from a surface, because no production surface consumes a
manifest yet.

## Catalog rules

### Construction

- **CR01.** One snapshot per host, built once by the composition root (UAK1
  layer `LR`) from static row providers. Nothing is registered, replaced or
  removed after construction.
- **CR02.** Tables are separately typed. There is no flat row enum spanning
  tables, no per-framework arm in a shared table type and no second registry.
- **CR03.** Registration carries no `Any`. Rows are concrete types; a frontend,
  backend or leg is a typed trait object of its capability trait. This is the
  successor state. Today `FrameworkRegistration` still carries three `Any`
  bridges reachable from the row: the surface-store bridges (`CAT-F04`,
  deleted with the surface stores by `D14`), the script-fact payload bridge
  reachable from `script_fact_providers` (`CAT-F05`, removed from the snapshot
  row by CPF1, session files only) and the carrier leg
  (`CAT-F06`, `CarrierLeg` plus `FrameworkAdapterCtx::carrier_for`, removed
  from the snapshot row by CPF1). The doc-hidden helpers
  `__carrier_downcast_ref` and `__carrier_downcast_arc` read the artifact's
  private carrier and do not read `FrameworkRegistration`.
- **CR04.** No runtime plugin loading and no dynamic native ABI. An external
  contribution enters only as a validated `T09` row; admitting it yields a new
  snapshot with a new identity and never mutates a built one.
- **CR07.** Duplicate-owner rejection. Two rows with one key, two carrier rows
  claiming one extension at the same precedence, two carrier rows sharing an
  editor language id, or a reference to a missing row fail construction with a
  typed error naming both owners. No row wins by order.
- **CR08.** Every carrier row has a frontend. A backend row is optional, and
  its absence is a normal state (UAK1 `U02`).

### Order and identity

- **CR05.** Each table is ordered by the canonical encoding of its row key.
  Provider order, call order and hash-map iteration never decide it.
- **CR06.** The snapshot identity (VID0 `I09`) is the canonical tagged
  encoding of the `ProfileSchemaEpoch` and the semantic fields of every table
  in canonical order. It never includes provider order, admission options,
  workspace or project state, revisions, or process/backend identity (VID0
  `R14`).
- **CR09.** Presentation order is row data (`T03` presentation rank),
  excluded from the identity and from dispatch. Generated mirrors render in
  presentation order, which keeps today's Vue-then-Svelte bytes.

### Use

- **CR10.** Consumers select rows by typed id from the snapshot or the
  `DemandPlan`. A display family, adapter spelling or extension is never a
  dispatch or cache key (VID0 `R03`, UAK1 `F04`).
- **CR11.** Admission and participation are not registration. Process flags,
  init options and editor settings never filter or re-mint the snapshot;
  choosing what runs is the `DemandPlan`'s (DEM0, COX0).
- **CR12.** Generated client mirrors are rendered only from snapshot rows, one
  generator per mirror, byte-pinned by a freshness test.
- **CR13.** Adding a family is a data change: rows in its own module and
  regenerated mirrors. Neutral routing code needs no new arm, branch or switch.
- **CR14.** Conflicting claims, static or contributed, resolve by a
  deterministic rule over row keys and declared relations, or yield a typed
  ambiguous or rejected outcome. First-loaded or fastest-returning never wins.

## External contribution slot

The `T09` slot is reserved now so the first external semantic or tool provider
needs no catalog change. Every contribution carries a namespace, an exact
contribution version, its provenance (`official-static`, `data-manifest` or
`isolated-guest`), the content digest of its admitted manifest and its target
ids. The reserved kinds are carrier (`XK01`), semantic profile (`XK02`),
project profile (`XK03`), embedded language (`XK04`), capability (`XK05`),
rule or action (`XK06`), tool operation (`XK07`), metadata facts (`XK08`) and
interoperability (`XK09`). An `XK09` contribution declares whether profiles
sharing a carrier are exclusive, coexisting or nested. Two relations on one
`T06` key fail construction (`CR07`); an overlap with no declared relation is
a typed ambiguous outcome (`CR14`). Admission order never chooses it.

Core still resolves every claim deterministically (`CR14`). A contribution
never replaces carrier parsing of a registered carrier, any identity, type
semantics, resolution and project membership, cache validity or edit
transactions. This contract defines the slot only. XSDK0 owns the extension
constitution (`XSDK0-AC1`, `XSDK0-AC2`) and XSDK1 the deterministic admission
into snapshots (`XSDK1-AC1`, `XSDK1-AC2`). The statically registered Vue and
Svelte profiles keep `official-static` provenance and do not become guests.

## Generated client manifests

The client manifest and the virtual-file naming mirror stay the single source
of the VS Code extension and TypeScript-plugin client wiring. At the CPF1
cutover their generators read `T01` and `T03` rows instead of joining
`descriptor.rs` with the extension table (`CAT-D04`), and their bytes do not
change (`MB01`). The freshness tests keep failing on a hand edit (`MB02`).
REG0 later splits each mirror into one generated file per family behind a
byte-equal index (`MB03`, `REG0-AC1`). The per-framework client branches that
remain are COX0's (`D10`); the per-family VS Code contributions are REG0's
(`C05`).

## Displaced routes recorded here

UAK0 and VID0 already own most routes this decision touches (`D01`–`D11`,
`D12`–`D14`, `D18`, `V-D01`–`V-D03`, `V-D05`, `K01`). These six are new. Each
has one production-capable deletion owner:

| Route | Unit | Disposition | Deletion owner |
| ----- | ---- | ----------- | -------------- |
| `CAT-D01` | `HostServices::composed` / `built_in` / `assert_classifier_agrees`: two registration authorities reconciled by runtime checks | replace: host construction takes the snapshot | CPF1 (`CPF1-AC1`) |
| `CAT-D02` | `FrameworkCapabilityCatalog::{compose, admitting, register_all}` | replace: `T01` rows registered at construction; admission leaves the catalog | CPF1 (`CPF1-AC1`) |
| `CAT-D03` | `ImmutableCapabilityCatalog<F, P, S, R, H>` as a catalog beside the snapshot | replace: `T01`/`T02` rows | CPF1 (`CPF1-AC1`) |
| `CAT-D04` | Mirror generators reading descriptors and the extension table | retag: same mirrors rendered from snapshot rows | CPF1 (`CPF1-AC1`) |
| `CAT-D05` | Mutable `RuleRegistry` keyed by adapter id | replace: immutable `T08` rows | LNT3 (`LNT3-AC1`) |
| `CAT-D06` | `ActiveProviderIndex` built inside the adapter registry as a selection authority | replace: gate data on `T03`, selection in the `DemandPlan` | COX0 (`COX0-AC1`) |

Coverage of the charter's deletion categories:

| Category | Recorded here | Referenced |
| -------- | ------------- | ---------- |
| central framework switch | `CAT-D06` | `D01`–`D07`, `D10`, `D11`, `V-D03` |
| untagged coordinate/public identity | none | `D08`, `D09`, `D16`, `D17`, `D19`, `V-D01`, `V-D02`, `V-D04`–`V-D06` |
| duplicate component information authority | none | `D12`–`D15`, `D18` |
| displaced registry constructor or generated mirror | `CAT-D01`–`CAT-D05` | `D02`, `D06`, `D07` |

The inventory also lists the registries that are not registration roots (the
external-TS `CarrierRegistry`, `ProjectRegistry`, `DocumentRegistry`,
`TaskRegistry`, `FactRegistry`, the type-engine registries,
`HtmlIntrinsicCatalog` and `RegisteredSourceAuthority`) with the reason each
stays out of scope.

## Equivalence fixtures

`registration-fixture-matrix.v1.json` fixes the observable outcomes the
cutover must preserve on every surface (registry, session, LSP, MCP, NAPI,
WASM, client) and the new rules' cases:

- `RM01` pins today's default registration on every surface; `RM02` keeps the
  Vue-only outcomes while the snapshot stays whole (`CR11`).
- `RM04`–`RM07` are the duplicate-owner and dangling-reference rejections.
- `RM08` is a carrier with no backend; `RM09` is the dormant row: an inert test
  family added through data and regenerated mirrors only, with no change to
  neutral routing.
- `RM10`–`RM12` cover capability rows and external contributions.
- `DM01`–`DM05` pin determinism: provider order and hash seeds do not matter,
  identity is stable across processes, excludes non-registration inputs, moves
  with every semantic change and ignores presentation order.

CPF1 executes the registration matrix and the duplicate-owner rejection
(`CPF1-AC-R2`). COX0, XSDK1, REG0 and VID0T own the cases the matrix assigns
to them.

## Findings recorded for the receiving owners

- **Admission composes a different catalog today** (`CAT-F01`, COX0). A
  Vue-only host builds a smaller capability catalog and registry. Under `CR11`
  the snapshot stays whole and the withdrawal moves to the `DemandPlan`. CPF1
  keeps today's typed `MalformedPayload` for a framework-surface request
  naming an unadmitted profile; changing it is COX0's decision (`RM02`).
- **Table order is provider order today** (`CAT-F02`, CPF1).
  `composed_rows_keep_the_frontend_catalog_order` pins the static frontend
  order; `DM01` replaces it.
- **REG0's grammar-key proposal conflicts with `CR02`** (`CAT-F03`, REG0).
  REG0's charter adds a `CarrierGrammarConfig::Framework(FrameworkCarrierKey)`
  arm with pre-allocated keys, while UAK0 `D01` has CPF1 replace the central
  enum with owner-local configs. REG0 must place its grammar keys in family
  row modules over the CPF1 shape.
- **Result caches hang off registration rows** (`CAT-F04`, TIF1).
  `FrameworkRegistration.surface_store` carries
  `ErasedFrameworkSurfaceStore::{as_any, into_any_arc}` and the stored bundle
  carries `FrameworkSurfaceDtoBundle::as_any`. A `T03` row carries no cache
  and no `Any` bridge; `D14` deletes the stores.
- **Script-fact payloads downcast from the registration row** (`CAT-F05`,
  CPF1, `CPF1-AC1`). `script_fact_providers` is a session registration leg.
  `crates/verter_session/src/framework/script_facts.rs` downcasts the payload
  with `as_any_arc`. CPF1 removes that leg from the snapshot row. The edit is
  those session files; CPF1's charter covers `crates/verter_session/src` and
  this row does not credit a `verter_semantic` edit. `D14` deletes surface
  stores only.
- **The carrier leg downcasts from the registration row** (`CAT-F06`, CPF1,
  `CPF1-AC1`). `FrameworkRegistration.carrier` is `Option<CarrierLeg>`, and
  `FrameworkAdapterCtx::carrier_for` downcasts the opened carrier with
  `CarrierParse::__verter_as_any_arc`. That use is reachable from the row, so
  it is in the CR03 population. CPF1 removes the leg from the snapshot row.
  The production paths are `framework/registry.rs` and `framework/ctx.rs`.

## Acceptance evidence

Evidence selection: the change adds contract text and data only, so existing
coverage and bounded inspection discriminate it. The diff adds no test.

- **AC1 — ownership contract.** The inventory binds every table, outcome,
  consumer and displaced route to one existing plan node, a successor path
  from CAT0 and a receiving acceptance ID; UAK0, UAK1 and VID0 rows are
  referenced, not re-owned. The executable validator is UAI0's
  (`UAI0-AC-R1`). It must accept `CN00` and refuse the six fixtures:
  known-non-production-owner (`CN01`: a docs-only owner with a valid
  node, path and acceptance ID, including findings rows `CAT-F05` and
  `CAT-F06`), unknown-owner (`CN02`), omission (`CN03`),
  path (`CN04`), acceptance (`CN05`) and conflict (`CN06`).
- **AC2 — positive contract.** Existing coverage pins the identities,
  provenance, completeness and ordering of today's registration:
  - `framework_registry_complete`,
    `host_classifier_and_composed_catalog_agree_on_carriers`,
    `register_all_rejects_two_rows_that_share_one_carrier_language`,
    `composition_fails_closed_on_a_row_without_a_registered_grammar` and
    `a_narrowed_admission_composes_only_the_admitted_verticals` in
    `verter_session` `framework::registry`;
  - the `framework::options` and `framework::language_classifier` unit tests
    in `verter_session`;
  - `client_framework_manifest_ts_freshness` and
    `virtual_file_naming_ts_freshness` in `crates/verter_session/tests/cases/`;
  - the `registry` and `carrier_grammar` unit tests and
    `crates/verter_language/tests/cases/` (`parse_identity`,
    `registered_authorities`, `sealed_block_identity`,
    `diagnostic_ordering`);
  - `typeinfo_proto_roundtrip` and `typeinfo_proto_ts_contract` in
    `crates/verter_protocol/tests/cases/`.

  New or extended tests belong to CPF1 (`CPF1-AC2`, `CPF1-AC-R2`).
- **AC3 — incremental equivalence: not applicable.** No cache, cancellation,
  stale-publication or partial-result authority is touched, and no production
  byte changes. The snapshot is built once per host and never invalidated;
  activation flips are FWA1's (`FWA1-AC5`).
- **AC4 — bounded work: not applicable.** No hot path changes. Zero work for
  inapplicable profiles stays UAK0's `Z01`, confirmed by PER0E; `RM09` binds
  the dormant row's zero work to CPF1.
