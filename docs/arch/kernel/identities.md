# Orthogonal identities and exact-release law

This decision makes syntax carrier, semantic release, attachment, project
profile, configured project, snapshot and capability independent typed
identities. It imports the stable source-unit lineage and the certified
TypeScript backend binding unchanged. Today, framework-shaped host/session
registries and untagged public boundaries own these identities. The final and
sole owner is the typed immutable universal catalog and the demand-selected
kernel services.

It describes the repository at `fix(ci): retry marketplace extension installs
in the VS Code E2E (#798)`, 2026-10-09. It follows the docs-only rule in
[README.md](README.md): it changes no production route and adds no check. It
builds on the [authority inventory](authority-inventory.md) and the
[constitution](constitution.md) and does not re-own anything they assign.

## Machine-readable products

The reviewed contract data lives in `tests/kernel/VID0/products/`:

| File | Holds |
| ---- | ----- |
| `identity-inventory.v1.json` | Identities `I01`–`I12` (each with its home paths and whether they exist yet), law rules `R01`–`R16`, outcomes `V-O01`–`V-O12`, consumers `V-C01`–`V-C11`, displaced routes `V-D01`–`V-D06`, the UAK0 routes it references, coverage of each deletion category, empty populations and transferred obligations |
| `identity-case-table.v1.json` | Cases `VC01`–`VC11`: input, required and forbidden outcome, the rules each case exercises, existing evidence, and the node whose test makes it executable |

Every `successorPath` starts at VID0 and follows predecessor edges in the
controller-owned plan. UAK0's `B..`, `O..`, `C..`, `S..` and `D..` rows keep
their owners; VID0 only refines or references them.

## Identity set

| Id | Identity | Axis | Disposition | Implementation owner |
| -- | -------- | ---- | ----------- | -------------------- |
| `I01` | `SourceUnitId` | stable lineage: `SourceId` plus logical role | imported (B4R0 repair, seam `S08`) | `verter_identity` |
| `I02` | `SourceRevision`, `ContentId`, `MapRevision` | exact version, bytes, map construction | imported | `verter_identity` |
| `I03` | `CarrierProfileId` | syntax carrier: bytes, geometry, parse, recovery | new | VID0T (`VID0T-AC1`) |
| `I04` | `ReleaseId` | one exact semantic release, one per manifest | new | VID0T (`VID0T-AC2`) |
| `I05` | `FrameworkProfileId` | meaning of one exact release over its carriers | new | VID0T (`VID0T-AC1`) |
| `I06` | `AttachmentId`, `RegionId` | one semantic claim on one addressed region of a unit | new | VID0T (`VID0T-AC1`) |
| `I07` | `ProjectProfileId` | project-profile overlay | new | VID0T (`VID0T-AC1`) |
| `I08` | `ConfiguredProjectId` | one configured project | new; derived by PM1 | VID0T (`VID0T-AC1`), PM1 (`PM1-AC1`) |
| `I09` | `CatalogSnapshot` identity | the one immutable registration catalog | new | CPF1 (`CPF1-AC2`; contract CAT0) |
| `I10` | `CapabilityId` | a capability, qualified by its profile | retag of the existing type | COX0 (`COX0-AC2`) |
| `I11` | `ProfileSchemaEpoch` | schema epoch of the descriptors behind `I03`–`I09` | new | VID0T (`VID0T-AC2`) |
| `I12` | `CertifiedTypeEngineBinding` | certified backend binding | imported unchanged (seam `S10`) | `verter_session` |

Each row in the inventory lists what the identity is derived from and what it
never contains. The short form:

- A **carrier** identity never contains a release, a semantic or project
  profile, a source unit, or a path or extension spelling.
- A **release** is exact. It never holds a range, a versions array, a
  floating tag or an implied major.
- A **semantic profile** holds exactly one release and the carriers it
  claims. It never holds a source unit or a project.
- An **attachment** is one profile's claim on one region. It survives edits
  that keep its region. Revision and content are never part of it. Competing
  claims are grouped by the region they share, never by attachment equality.
- A **project profile** references the semantic profiles it overlays and never
  re-mints them. A **configured project** is a separate identity and never
  carries a profile.
- **Backend and process identity** belongs to the binding. No successor
  `BackendInstanceEpoch` or provider epoch is defined beside it; the existing
  `ProviderEpoch` and `EngineIdentity` remain its serving facts.

## Law

### Orthogonality

- **R01.** Every identity is a distinct nominal type: no type alias, shared
  base or implicit conversion.
- **R02.** Each identity is minted only from the inputs its row names.
  Changing one input changes exactly the identities derived from it, directly
  or through another identity, and nothing else. A new release yields new
  semantic profile, attachment, capability and catalog identities; the carrier,
  source unit and region stay equal (`VC11`). VID0T proves the release,
  profile and attachment half; COX0 (`COX0-AC2`) proves the capability change
  and CPF1 (`CPF1-AC2`) the catalog change.
- **R03.** A display family (a `FrameworkTag` value, an adapter spelling, a
  file extension) is presentation only. It is never a dispatch key or a cache
  key.

### Exact release

- **R04.** One manifest declares exactly one release. Two releases, or a
  versions array, in one manifest is a structural failure.
- **R05.** Separate majors are separate manifests and separate semantic
  profiles. They never share a profile, a cache key or a vocabulary table.
- **R06.** There is no implicit default major. A release that does not resolve
  to an exact admitted release is `unsupported-version` and is never coerced.
- **R07.** `latest`, `next`, canary and other floating tags, ranges and untagged
  strings never decode into a release or a profile identity.
- **R08.** Activation reads the resolved installed version, never the declared
  range.

### Workspace multi-version resolution

- **R09.** Each package resolves its own release. Two packages in one
  workspace may hold different releases of one family, each with its own
  profile identity and cache entries.
- **R10.** Ambiguity is decided per region, keyed by the profile-independent
  `(SourceUnitId, RegionId)`. When two active profiles each claim the same
  region (two distinct `AttachmentId`s) and no per-file narrowing selects one,
  the outcome is typed ambiguous. Registration order, load order or recency
  never decides.
- **R11.** A family whose admitted line is one major admits only that major.
  Qwik admits Qwik 2 only. A Qwik 1 package is `unsupported-version`, so
  Qwik-2-only rules need no version gate inside the profile. QWK0 fixes the
  release; its executable rejection proof is QWK1's (`QWK1-ACV`, `QWK1-AC4`).

### Serialization and collision

- **R12.** Cache and audit keys over source text use
  `(SourceUnitId, SourceRevision, ContentId)`, or
  `(SourceUnitId, SourceRevision, MapRevision)` for maps, through the canonical
  tagged, length-delimited encoding.
- **R13.** Collision safety comes from the encoding. Two tuples whose
  concatenated payloads coincide still encode differently, and a suspected
  digest collision compares full canonical bytes.
- **R14.** Backend and process identity never enters a source, carrier,
  profile or catalog key.
- **R15.** Syntax artifacts are keyed by carrier profile. Semantic caches are
  keyed by the exact semantic profile they compute under. A family-wide key is
  forbidden.

### Placement

- **R16.** Profile identities are not threaded into `HostConfig`,
  `CompileProfile` or `CodegenOptions`. Consumers read them from the catalog
  and the demand plan.

## `FileLanguage` migration

`FileLanguage::Framework { adapter_id, language_id }`,
`FileLanguage::FrameworkTemplate` and `ScriptFlavor::AdapterModule` conflate a
carrier with a semantic claim and identify both by open strings. UAK0 already
assigns their deletion to CPF1 (`D08`) and their consumers to CPF1 (`C02`,
`C03`). This decision fixes the split CPF1 implements:

- the producers (`LanguageRegistry::classify_static`,
  `HostLanguageClassifier::classify_registered`) yield a `CarrierProfileId`;
- the semantic claim moves to DEM0's claim plan, implemented by COX0;
- the cache keys that carry a `FileLanguage` row (`FileArtifactKey`,
  `SourceEnvIdentity`, the framework script-fact `CandidateSlotKey`) follow
  `R15` (`V-C01`; it also covers the `FrameworkArtifactId` and
  `CarrierParseKey` adapter/language fields and the `FileSourceEnv` fact);
- the LSP's own `FileLanguage` producers and consumers, document
  classification included, move in the same CPF1 change (`V-C10`): the LSP
  returns the host classifier's `FileLanguage` directly, so they cannot wait
  for another landing. Per-profile editor participation stays COX0's (UAK0
  `C06`, `D10`).

All consumers move in one change, and the conflated ids are deleted in that
same change.

## Displaced routes recorded here

UAK0's routes do not cover these six. Each has one production-capable
deletion owner.

| Route | Unit | Disposition | Deletion owner |
| ----- | ---- | ----------- | -------------- |
| `V-D01` | Implicit default major: an adapter identity names a family and silently means its current major; descriptors carry no release | replace | FWA1 (`FWA1-AC1`) |
| `V-D02` | Open `ComponentSelector.framework_adapter_id` string in the public TypeInfo request | replace | TIF1 (`TIF1-AC1`) |
| `V-D03` | Wire `FrameworkTag` used as an adapter identity in `tag_disposition` | retag: the tag stays a display family; the exact profile travels beside it | REG0 (`REG0-AC2`) |
| `V-D04` | Four producer-specific `SourceId` domains for one carrier file (compiler assembly logical source, Vue main assembly, Svelte main assembly, Vue custom blocks) | replace with one minter per logical source | CPF1 (`CPF1-AC1`) |
| `V-D05` | Svelte release admission by string prefix (`major == "5"`) in the LSP asset loader | replace: the loader reads the admitted release from the activation record | FWA1 (`FWA1-AC1`) |
| `V-D06` | Untyped `ProjectStableKey::{Configured, Fallback}(Hash16)` as the cross-snapshot project key | retag to `ConfiguredProjectId`; the fallback arm becomes PM1's inferred-project identity | PM1 (`PM1-AC-R1`: the path-derived key is a planted path-only identity) |

The inventory references UAK0's routes for the identity concerns they already
own, and maps every deletion category this decision must inventory:

| Category | Recorded here | Referenced UAK0 routes |
| -------- | ------------- | ---------------------- |
| central framework switch | `V-D03` | `D05`–`D07` (CPF1) |
| untagged coordinate/public identity | `V-D01`, `V-D02`, `V-D04`–`V-D06` | `D08` (CPF1), `D09` (COX0), `D19` (PM1) |
| duplicate component information authority | none | `D12`–`D15` (TIF1), `D18` (IDX0) |

The duplicate component information routes already carry their identity
concern: the component-meta, public-API and framework-surface authorities
(`D12`–`D14`) are replaced by one TypeInfo view selected by the typed semantic
profile, so `D14`'s family-wide surface keys fall under `R15`; `D15` is the
request vocabulary whose open selector string is `V-D02`; and `D18`'s
replacement index contributions are keyed by profile and attachment
(`V-C07`).

Empty populations at this head:

- **Framework release identity in identities, keys and wire types.** No
  identity, cache key or proto field carries a framework release, so there is
  no versions array or family-wide release key to delete. Two LSP loaders do
  read a framework package version: `svelte_assets` admits Svelte by string
  prefix (`V-D05`), and `vue_assets` folds the exact version into an asset
  key (`V-C11`, which already obeys `R15`). The Svelte runes/legacy choice is
  a per-file mode inside one release. `V-D01` records the implicit major.
- **A successor backend epoch.** No `BackendInstanceEpoch` exists.

## Findings recorded for the receiving owners

- **Source lineage.** The B4R0 repair holds at the constructor:
  `SourceUnitId` is minted only by `from_lineage`, and revision and content are
  neighbours. But one `.vue` file can get up to four different `SourceId`s,
  depending on which compiler product mints it (`V-D04`). The custom-block
  domain also folds in the file incarnation, so its lineage changes on reopen.
  Lineage does not survive across products until CPF1 consolidates the minter.
- **Two project keys.** `ProjectStableKey` already gives configured projects a
  stable cross-snapshot key, outside the canonical encoding. `ConfiguredProjectId`
  replaces it rather than standing beside it (`V-D06`). `ProjectId(u32)` stays a
  positional per-snapshot index and is not an identity.
- **Binding project key.** `CertifiedTypeEngineBinding` holds its project as
  an `Arc<str>`. VID0 imports the binding unchanged. PM1 decides whether the
  derived `ConfiguredProjectId` feeds `BoundProject`; the binding's own fields
  stay binding-owned.
- **No Vue 2 vertical.** `VC02` pins Vue 2.6 and Vue 3 coexistence at the
  identity level only. The Vue 2 migration reader node was abandoned, so no
  Vue 2 profile is admitted for activation.

## Acceptance evidence

Evidence selection: the change adds contract text and data only, so existing
coverage and bounded inspection discriminate it. The diff adds no test.

- **AC1 — ownership contract.** The inventory binds every identity, outcome,
  consumer and displaced route to one existing plan node, a successor path from
  VID0 and a receiving acceptance ID. UAK0 and UAK1 rows are referenced, not
  re-owned. The executable validator and its negative controls are UAI0's
  (`UAI0-AC-R1`).
- **AC2 — positive contract.** Existing coverage already pins the imported
  identities and the boundaries this decision names:
  - lineage and encoding in `verter_identity`:
    `source_unit_lineage_is_stable_across_revision_and_content`,
    `golden_bytes_are_pinned`,
    `domain_tag_separates_otherwise_identical_payloads`,
    `field_order_is_significant`, and the compile-fail fixtures run by `verter_compile_contracts` (including
    `source_unit_id_has_no_from_canonical`);
  - `content_and_revision_do_not_change_source_unit_id` in
    `verter_compiler` `assembly::source_unit`;
  - `crates/verter_language/tests/cases/` (`parse_identity`,
    `registered_authorities`, `sealed_block_identity`, `diagnostic_ordering`);
  - `typeinfo_proto_roundtrip` and `typeinfo_proto_ts_contract` in
    `crates/verter_protocol/tests/cases/`;
  - `framework_registry_complete`,
    `framework_surface_wire_executor_validates_first` and the
    `typeinfo_request_validation` cases in `verter_session`.

  New tests for `I03`–`I11` and the case table belong to VID0T.
- **AC3 — incremental equivalence: not applicable.** No cache, cancellation,
  stale-publication or partial-result authority is touched, and no production
  byte changes. `R09` and `VC10` bind the later incremental proof to FWA1
  (`FWA1-AC5`).
- **AC4 — bounded work: not applicable.** No hot path changes. Zero work for
  inapplicable profiles stays UAK0's `Z01`, confirmed by PER0E.
