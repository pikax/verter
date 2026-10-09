# Selection, two-stage activation and demand planning

This decision defines how Verter chooses what runs. Captured selection inputs
feed a pre-projection `SourceActivationPlan` (stage one), a post-snapshot
`SemanticClaimPlan` (stage two) and a per-request `CapabilityDemandPlan`. The
three compose into one `DemandPlan`, which is the only answer to "which
profile, which capability and which facts run for this request". Supported
profiles stay dormant until they are proven by captured facts and requested by
an operation.

Today, framework-shaped host/session registries and untagged public boundaries
own selection: one framework per file chosen by extension, the process-wide
`--frameworks` admission, the provider index built inside the adapter
registry, and per-framework branches in the LSP and MCP. The final and sole
owner is the typed immutable universal catalog and the demand-selected kernel
services.

It describes the repository at `docs(arch): define the declarative
configuration envelope and captured (#809)`, 2026-10-09. It follows the
docs-only rule in [README.md](README.md): it changes no production route and
adds no check. It reads the [catalog](catalog.md), [identities](identities.md)
and [configuration](configuration.md) decisions, and does not re-own anything
they or the [authority inventory](authority-inventory.md) and the
[constitution](constitution.md) assign.

## Machine-readable products

The reviewed contract data lives in `tests/kernel/DEM0/products/`:

| File | Holds |
| ---- | ----- |
| `demand-inventory.v1.json` | Plan stages `P1`–`P5`, contract rules `DR01`–`DR30`, outcomes `DEM-O01`–`DEM-O10`, consumers `DEM-C01`–`DEM-C08`, displaced routes `DEM-D01`–`DEM-D06`, the UAK0, VID0, CAT0 and CFG0 routes it references, coverage of each deletion category, empty populations, findings and transferred obligations |
| `demand-case-table.v1.json` | Cases `DC01`–`DC16`: input, required and forbidden outcome with exact work and audit counts, the rules each case exercises, existing evidence, and the node whose test makes it executable |

Every `successorPath` starts at DEM0 and follows successor edges in the
controller-owned plan. UAK0, VID0, CAT0 and CFG0 rows keep their owners; DEM0
only references them.

## Plan stages

| Stage | Name | Computed from | Fixed for | Implementation owner |
| ----- | ---- | ------------- | --------- | -------------------- |
| `P1` | `SelectionInputs` | captured inputs only (`DR01`–`DR05`) | one activation epoch | COX0 (`COX0-AC2`) |
| `P2` | `SourceActivationPlan` | `P1` plus parse-domain facts of one source revision | one parse/transform generation | COX0 (`COX0-AC2`) |
| `P3` | `SemanticClaimPlan` | `P2` plus the published project snapshot | one project snapshot | COX0 (`COX0-AC2`) |
| `P4` | `CapabilityDemandPlan` | `P3` plus one request's operation, root and participation mask | one request | COX0 (`COX0-AC2`) |
| `P5` | `DemandPlan` | the composition of `P1`–`P4` and its submission | one request | COX0 (`COX0-AC2`) |

UAK0 `O03` already assigns "demand-selected activation and per-profile
capability masks" to COX0, and the authority inventory binds `DemandPlan` to
DEM0 → COX0. This decision splits that outcome into the stages above; it does
not move it. FWA1 produces the `FrameworkActivation` record that `P1` reads,
EAK1 produces the role evidence `P2` and `P3` read, and TIF1, CMP1 and IDX0
consume the plan.

## Contract rules

### Captured selection inputs (subblock 1)

- **DR01.** `SelectionInputs` is a closed, captured tuple of three members:
  the `CatalogSnapshot` identity (VID0 `I09`); the effective configuration's
  `frameworks` section for the scope, with its value fingerprint (CFG0
  `CR14`, `CR16`); and the `FrameworkActivation` records of the owning
  package, published per PM snapshot (FWA1). Nothing else activates. The
  host's declared client capabilities are not a member and have no
  activation identity. COX0D owns that vocabulary — provider unavailable,
  unsupported client feature, and disabled optional visual layer — inside
  the participation mask. The mask, mode and those capabilities enter at
  `P4` (`DR15`); their identity is the mask identity, so a change mints a
  new demand epoch only (`DR21`). Standing down never changes what is
  activated or parsed.
- **DR02.** Every member is read through its owner's captured snapshot.
  Selection reads no file, package manifest, lockfile, environment variable,
  process flag or editor setting directly. A member that is not captured is
  `NeedInputs` (CFG0 `CR23`), never treated as absent.
- **DR03.** Spelling never activates. A file extension, an import specifier
  string, an adapter spelling, a file name probe (`nuxt.config.*`) or a
  display family is not a selection input (VID0 `R03`, CAT0 `CR10`). Only
  decoded typed identities and owner-proven facts are.
- **DR04.** Process-wide admission is not selection. `--frameworks`,
  `FrameworkOptions` and the NAPI/WASM `frameworks` option never filter the
  catalog (CAT0 `CR11`). Their successor is the `frameworks` section, which a
  host supplies through the user layer (CFG0 `CR05`, `CR22`).
- **DR05.** The selection-input identity is the canonical tagged encoding
  (VID0 `R12`, `R13`) of each member's identity in a fixed member order. It
  carries no path string and no backend or process identity (VID0 `R14`).
  The configuration member is the `frameworks` value fingerprint only, not
  the whole effective configuration and not CFG0 `CR14`'s consulted-row
  query key. The catalog member is the whole `CatalogSnapshot` identity
  (VID0 `I09`): a catalog re-version mints a new identity even when the
  rows a decode consulted are unchanged.

### Pre-projection `SourceActivationPlan` (subblock 2)

- **DR06.** Stage one runs per `(SourceUnitId, SourceRevision, ContentId)`
  before any framework projection, script-fact capture or TypeInfo work. It
  selects the unit's `CarrierProfileId` from the catalog's `T01` claim and,
  per region, the candidate `FrameworkProfileId`s that the package's
  activation records and per-file narrowing allow.
- **DR07.** Stage one may read only `SelectionInputs` and parse-domain facts
  of the same revision: the carrier's region inventory, a
  `@jsxImportSource` pragma, and EAK1's projection-time role evidence from
  canonical package exports. It never calls TypeScript, the shared type
  resolver, TypeInfo or any semantic oracle.
- **DR08.** Each region's stage-one outcome is exactly one of `Selected
  { profile }`, `Dormant { reason }`, `Ambiguous { claims }` or `Unproven
  { needs }`. `reason` is one of `Off`, `Inactive`, `UnsupportedVersion` or
  `NotClaimed`; the first three come from the activation record (FWA1).
- **DR09.** The stage-one plan fixes the parse/transform generation. Its
  identity is `(SourceUnitId, SourceRevision, ContentId, CarrierProfileId,
  ordered per-region outcomes, selection-input identity)`. Syntax artifacts
  stay keyed by carrier profile (VID0 `R15`); a profile outcome enters an
  artifact key only for artifacts the profile computes.
- **DR10.** A carrier that registers no profile claim (plain `ts`, `js`) has
  an empty region outcome set, and stage one does no work beyond reading the
  catalog row.

### Post-snapshot `SemanticClaimPlan` (subblock 3)

- **DR11.** Stage two runs after the project snapshot that contains the
  stage-one generation is published. It mints one `AttachmentId` per
  `(SourceUnitId, RegionId, FrameworkProfileId)` for every `Selected` region
  (VID0 `I06`), using post-snapshot facts: resolved imports, canonical role
  provenance that needs resolution, and project membership (PM1).
- **DR12.** Post-snapshot facts never mutate the current parse/transform
  generation. Stage two can confirm a stage-one selection or withdraw it for
  that snapshot (`Withdrawn { cause }`); it can never add a profile, change
  the carrier, or re-run a stage-one projection under the same generation.
- **DR13.** When a post-snapshot fact would change a stage-one input (for
  example a dependency edit that flips an activation record), the change
  reaches stage one only as a new `SelectionInputs` and a new activation
  epoch (`DR21`). The old generation is never edited in place.
- **DR14.** A semantic claim names its project profile when one applies
  (VID0 `I07`, PPR0T rows) and never re-mints the semantic profile it
  overlays.

### Capability-level `CapabilityDemandPlan` (subblock 4)

- **DR15.** A request names one operation descriptor and one normalized root
  context, taken from the READS envelope (SKR-READS) and, for TypeInfo, the
  TIF0 operation descriptors. Stage four intersects the operation's
  capability cells (VID0 `I10`, profile-qualified) with the confirmed claims
  of `P3` and with the participation mask (COX0D). That mask is the identity
  of the COX0D mode and of the host's declared client capabilities. Neither
  is a `SelectionInputs` member and neither enters the activation epoch.
- **DR16.** Every capability declares its exact fact demands: the fact kinds
  and query families it reads, keyed by profile. A capability that cannot
  state them is a construction failure of the catalog row, never a capability
  that runs everything. Demand never expands to "all capabilities of the
  profile" or "all profiles of the file".
- **DR17.** `DemandPlan` carries a closed demand purpose: `Interactive`,
  `DiagnosticsPublication`, `BatchCheck`, `IndexContribution`, `Compile` or
  `Explain`. Purpose is part of the plan identity and selects the budget
  profile (SKR-READS `BudgetProfile`). It never selects a different semantic
  answer.
- **DR18.** Batch membership is the set of `(SourceUnitId, RegionId,
  CapabilityId, fact demand)` entries. Batch order is the canonical encoding
  order of those entries (CAT0 `CR05`), never arrival, discovery or
  completion order.
- **DR19.** Profile identity for refusal reuse is `(demand purpose,
  BudgetProfile identity, ordered FrameworkProfileId set, normalized root
  context, demand epoch, root source identity)`. The demand epoch is the
  `DR21` epoch. The root source identity is VID0 `R12`: `(SourceUnitId,
  SourceRevision, ContentId)`, or `(SourceUnitId, SourceRevision,
  MapRevision)` when the root is a map. An eligible isolated-root refusal is
  reused only under that exact identity. A refusal warmed under one demand
  epoch or one root source identity is not reusable under another (`DR25`,
  `DC05`). It stays a refusal and is never read as exact absence (TIF0
  2026-09-30 amendment, including that amendment's edit isolation for a
  reused refusal). A dependency admitted under a root needs no second root
  permit. A request with capture off performs no observation-only work.
- **DR20.** `DemandPlan` is submitted through the engine-owned
  `ExecutionSubmission` port: `attach_engine()` returns an
  `EngineBinding<MacroMirrors>` holding engine resources only. Host-owned
  stores travel through `HostAttachmentPort`, never through the binding or the
  plan. UAO0 re-validates purpose, batch membership and order and refusal
  reuse against SKR-PARALLEL's final submission semantics (`UAO0-AC-R3`); a
  mismatch returns here as an amendment.

### Conflict, ambiguity and epoch transitions (subblock 5)

- **DR21.** An activation epoch is the selection-input identity. Its closed
  member list is `DR01`'s three: the `frameworks` value fingerprint, the
  activation record and the catalog identity. Any change to one of those
  members mints a new activation epoch. A demand epoch is the activation
  epoch plus the participation mode and mask identity. The host's declared
  client capabilities are inside that mask, so a change to them mints a new
  demand epoch only, as does any other participation change. A change that
  leaves every member identity unchanged mints neither (CFG0 `CR16`).
- **DR22.** Ambiguity is decided per region, keyed by `(SourceUnitId,
  RegionId)` (VID0 `R10`). Two active profiles claiming one region with no
  per-file narrowing and no declared `T06` relation is `Ambiguous { claims }`
  (CAT0 `CR14`). Registration, load, discovery and completion order never
  decide. An ambiguous region serves no profile-specific result for that
  region and reports the ambiguity once per activation epoch. A
  participation-only change mints a demand epoch (`DR21`) and does not
  report that ambiguity again.
- **DR23.** A declared `T06` relation decides deterministically: `exclusive`
  keeps the ambiguity, `coexisting` selects both claims on disjoint
  capabilities, `nested` selects the outer claim for the region and the inner
  claim for its child regions.
- **DR24.** A missing package or an unresolved installed version is
  `Unproven { needs }`. It is never `Inactive`, never cached as a warm
  negative, and never replaced by a name-based guess (FWA1 abort). When PM
  later proves the version, a new epoch selects or rejects the profile.
- **DR25.** Rapid mode changes, for either epoch: work admitted under an
  older epoch completes `ReturnOnly` or is cancelled. It is never published warm and never served
  after a newer epoch has been observed. Work not yet admitted when its epoch
  is superseded is never admitted. Only the newest epoch's plan runs.

### Zero work and cancellation (subblock 6)

- **DR26.** A `Dormant`, `Ambiguous`, `Unproven` or `Withdrawn` region
  performs zero framework projection, script-fact, TypeInfo and index work for
  that profile (UAK0 `Z01`).
- **DR27.** A selected but unrequested profile performs zero capability work.
  The carrier parse runs only when another demand needs the carrier (UAK0
  `Z02`); selection alone never triggers it.
- **DR28.** Activation and demand planning emit exactly one audit record per
  plan, not one per epoch. A stage-one plan is one source unit under one
  activation epoch (`DR06`, `DR09`), so that epoch emits one
  `ActivationPlanned { epoch, selected, dormant, ambiguous, unproven }` for
  each source unit it plans, and the counts are that plan's regions only.
  Stage four emits one `DemandPlanned { purpose, capabilities, facts }` per
  demand plan (one request or one batch). Dormant work is counted, never
  executed. Audit capture is optional detail; required plan state survives
  capture off (UAO0 2026-09-30 amendment).
- **DR29.** Cancellation is observed at plan boundaries and at the engine's
  `CancellationCheckpoint`. A cancelled plan publishes nothing, leaves no
  warm entry and no refusal, and a retry answers what a fresh host answers.
- **DR30.** Degraded outcomes (`Unproven`, `Ambiguous`, cancelled,
  superseded, budget-exceeded) are `ReturnOnly`. They never warm a plan cache
  or a result cache.

## Outcomes and owners

| Outcome | Owner | Receiving acceptance |
| ------- | ----- | -------------------- |
| `DEM-O01` captured `SelectionInputs` and its identity (`DR01`–`DR05`) | COX0 | `COX0-AC2` |
| `DEM-O02` pre-projection `SourceActivationPlan` (`DR06`–`DR10`) | COX0 | `COX0-AC2` |
| `DEM-O03` post-snapshot `SemanticClaimPlan` (`DR11`–`DR14`) | COX0 | `COX0-AC2` |
| `DEM-O04` `CapabilityDemandPlan` with exact fact demands (`DR15`, `DR16`) | COX0 | `COX0-AC2` |
| `DEM-O05` `DemandPlan` purpose, batch order, refusal-reuse identity and submission (`DR17`–`DR20`) | COX0 | `COX0-AC2` |
| `DEM-O06` ambiguity resolution and epoch transitions (`DR21`–`DR25`) | COX0 | `COX0-AC3` |
| `DEM-O07` zero work, audit and cancellation (`DR26`–`DR30`) | COX0 | `COX0-AC4` |
| `DEM-O08` `FrameworkActivation` records consumed by `P1` | FWA1 | `FWA1-AC1` |
| `DEM-O09` canonical role evidence consumed by `P2` and `P3` | EAK1 | `EAK1-AC2` |
| `DEM-O10` executable ownership validator and fixtures for this decision | UAO0 | `UAO0-AC-R1`, `UAO0-AC-R2` |

## Displaced routes recorded here

UAK0, VID0, CAT0 and CFG0 do not cover these six. Each has one
production-capable deletion owner. Five belong to the population the
charter's reconciled contract removes after parity: legacy eager and
one-framework-per-file selectors.

| Route | Unit | Disposition | Deletion owner |
| ----- | ---- | ----------- | -------------- |
| `DEM-D01` | LSP diagnostics publication (`compute_verter_diagnostics_for_with_views`) runs every producer on each publish, including an eager `get_component_meta` for projection-limit diagnostics, whatever the request or participation | replace with a `DiagnosticsPublication` plan whose capabilities come from the participation mask | COX0 (`COX0-AC1`) |
| `DEM-D02` | Admission-driven reclassification in `HostLanguageClassifier`: an unadmitted vertical's carrier routes like an unregistered extension, and its adapter modules become plain scripts | replace: the carrier keeps its `CarrierProfileId` and the profile is `Dormant` | COX0 (`COX0-AC1`) |
| `DEM-D03` | The framework-surface executor plans and resolves every surface kind (`ALL_FRAMEWORK_SURFACE_KINDS`); the wire has no requested-kind demand | replace with requested facets through `P4` | TIF1 (`TIF1-AC1`) |
| `DEM-D04` | Spelling-based role recognition in the component-meta resolver: `should_ignore_external_macro_type` compares `import_source == "vue"` | replace with canonical role evidence | EAK1 (`EAK1-AC1`) |
| `DEM-D05` | Nuxt server/client component detection by suffix (`is_ssr_file`, `is_client_only_file`: `.server.vue`, `.client.vue`) | replace with Nuxt profile applicability from captured facts | NUX0 (`NUX0-AC1`) |
| `DEM-D06` | `handle_did_open` and `handle_did_change` schedule full native semantic enrichment, and `handle_did_open` prewarms imported-carrier APIs, with no participation or demand gate | replace with an `Interactive` or `DiagnosticsPublication` plan | COX0 (`COX0-AC1`) |

DEM0 references these routes owned elsewhere:

| Route | Owner | Concern |
| ----- | ----- | ------- |
| UAK0 `D09` | COX0 | Process-flag admission and untagged `CapabilityId`; `DR04` names the successor |
| UAK0 `D10`, `D11` | COX0 | Per-framework LSP branches and MCP `is_vue()` gates |
| UAK0 `D06`, `D07`, `D08` | CPF1 | Hard-coded admitting registrations, the extension table that picks one framework per file, and the conflated `FileLanguage` |
| UAK0 `D12`, `D14` | TIF1 | Component-information authorities this decision only demands from |
| UAK0 `D18` | IDX0 | The per-request workspace component scan |
| VID0 `V-D01`, `V-D05` | FWA1 | Implicit default major and string-prefix release admission |
| VID0 `V-D02` | TIF1 | The open `framework_adapter_id` selector string |
| CAT0 `CAT-D01`, `CAT-D02` | CPF1 | Admission-composed catalog and registry |
| CAT0 `CAT-D06` | COX0 | `ActiveProviderIndex` as a per-file selection authority |
| CFG0 `CF-D05` | NUX0 | `detect_ssr_project` and `detectNuxt` file-name probes |

Deletion-category coverage:

| Category | Recorded here | Referenced |
| -------- | ------------- | ---------- |
| central framework switch | `DEM-D04` | `D06`, `D07`, `D10`, `D11`, `CAT-D01`, `CAT-D02`, `CAT-D06`, `CF-D05` |
| untagged coordinate/public identity | none: selection adds no public identity route | `D08`, `D09`, `V-D01`, `V-D02`, `V-D05` |
| duplicate component information authority | none: `DEM-D01` and `DEM-D03` are eager demands on existing authorities | `D12`, `D14`, `D18` |
| eager or one-framework-per-file selector (this decision's own population) | `DEM-D01`–`DEM-D03`, `DEM-D05`, `DEM-D06` | — |

The inventory also lists, under `referencedConsumers`, the consumers that
UAK0, VID0, CAT0 and CFG0 already own (`C06`–`C09`, `V-C04`, `V-C06`,
`CAT-C02`, `CF-C01`). DEM0 adds only consumers they do not list:

| Consumer | Reads | Owner |
| -------- | ----- | ----- |
| `DEM-C01` session script-fact resolution and synth-path provider selection | `P2`, `P4` | COX0 (`COX0-AC2`) |
| `DEM-C02` framework-surface executor and the `TypeInfoRequest` entry | `P3`, `P4` | TIF1 (`TIF1-AC2`) |
| `DEM-C03` compiler requests over demanded fact families | `P4` | CMP1 (`CMP1-AC2`) |
| `DEM-C04` workspace index contributions (`IndexContribution` purpose) | `P3`, `P5` | IDX0 (`IDX0-AC2`) |
| `DEM-C05` Nuxt project-profile applicability on semantic claims | `P3` | NUX0 (`NUX0-AC1`) |
| `DEM-C06` LSP workspace provider sync and carrier publication (`BatchCheck` purpose) | `P5` | COX0 (`COX0-AC2`) |
| `DEM-C07` host audit runtime receiving the `DR28` records | `P2`, `P4` | COX0 (`COX0-AC4`) |
| `DEM-C08` embedded-language region geometry under nested claims | `P3` | EMB0I (`EMB0I-AC1`) |

## Findings recorded for the receiving owners

- **The capability snapshot is never filled** (`DEM-F01`, FWA1). Host
  construction and the default classifier pass
  `ProjectCapabilitySnapshot::empty()`, so every gated classifier row takes
  its fallback. FWA1's activation record is the first producer.
- **An import-specifier gate exists without a production user**
  (`DEM-F02`, COX0). `ScriptFactSyntaxGate::ImportSpecifier` and
  `ActiveProviderIndex.by_import_specifier` select by specifier spelling;
  only tests use them. `DR03` forbids that as a selection input. Removing it
  from selection belongs to `CAT-D06`.
- **Component-meta decides a carrier by path suffix** (`DEM-F03`, TIF1).
  `declaration.canonical_source.ends_with(".vue")` gates direct macro type
  references. It is part of the `D12` population.
- **The submission port exists** (`DEM-F04`, UAO0). `ExecutionSubmission`,
  `attach_engine` and `EngineBinding` live in
  `crates/verter_type_engine/src/resolver_core/request_ports.rs` and
  `project_semantic_dispatch/engine_binding.rs`. The production impl is
  `RequestBoundAdapter`; the `VerterHost` impl is test-support only. `DR20`
  drafts against this port, and UAO0 re-validates it (`UAO0-AC-R3`).
- **MCP selection is the classifier's answer** (`DEM-F05`, TIF1).
  `get_framework_surface` sends
  `language_classifier().classify(canonical).adapter_id()` as a string
  (`V-D02`).
- **Workspace-wide provider sync is a declared demand, not a displaced
  route.** `eager_sync_real_sources` and
  `background_publish_workspace_carriers` feed the project-bound TypeScript
  program. They stay, declared under the `BatchCheck` purpose (`DEM-C06`,
  COX0).

## Empty populations

- **No successor plan types.** No `CarrierProfileId`,
  `FrameworkProfileId`, `ProjectProfileId`, `CatalogSnapshot`,
  `DemandPlan`, `SourceActivationPlan`, `SemanticClaimPlan` or
  `CapabilityDemandPlan` exists under `crates/`.
- **No activation record.** `framework/project_capabilities.rs` holds only
  derived capability bits (`DEM-F01`).
- **No post-snapshot claim stage.** Classification is one static step, so
  `P3` displaces nothing.
- **No activation or demand audit.** The script-fact miss paths are typed
  outcomes (`ProviderNotRegistered`, `ProviderGateMiss`), not counters.
- **No per-region ambiguity.** `classify_static` returns one `FileLanguage`
  per path, so two profiles never claim one region today.

## Acceptance evidence

Evidence selection: the change adds contract text and data only, so existing
coverage and bounded inspection discriminate it. The diff adds no test.

- **AC1 — ownership contract.** The inventory binds every plan stage, rule,
  outcome, consumer and displaced route to one existing plan node, a
  successor path from DEM0 and a receiving acceptance ID. UAK0, VID0, CAT0
  and CFG0 rows are referenced, not re-owned. The executable validator is
  UAO0's (`UAO0-AC-R1`). The fixtures `DC01`–`DC05` (disabled,
  selected-but-unrequested, ambiguous, missing-package, rapid-mode-change)
  are UAO0's too (`UAO0-AC-R2`), and so is the re-validation of
  `DR17`–`DR20` against SKR-PARALLEL (`UAO0-AC-R3`). Every other case names
  its own executable owner.
- **AC2 — positive contract.** Existing coverage pins the identities,
  provenance, completeness and ordering of today's selection, which the
  stages replace:
  - admission and composition: `unknown_name_is_rejected_naming_the_supported_set`
    in `verter_session` `framework::options`;
    `a_narrowed_admission_composes_only_the_admitted_verticals`,
    `framework_registry_complete` and
    `built_in_active_provider_index_gates_svelte_only` in
    `framework::registry`;
    `an_unadmitted_verticals_sources_fail_closed_on_the_real_host` in
    `host_construction`; `wasm_frameworks_key_narrows_the_constructed_host`
    in `verter_wasm`;
  - classification: `an_unadmitted_vertical_is_invisible_to_classification`
    and the other `framework::language_classifier` unit tests;
  - provider gating: `script_fact_providers_zero_cost_on_miss` and
    `script_fact_capture_is_syntax_only` in
    `crates/verter_session/tests/cases/g_misc0/framework_adapter_guards.rs`,
    and `no_provider_registration_is_zero_cost_not_applicable`;
  - the request boundary: `framework_surface_wire_executor_validates_first`,
    `unknown_adapter_id_returns_malformed_payload` and the
    `typeinfo_request_validation` cases in `verter_session`;
    `typeinfo_proto_roundtrip` and `typeinfo_proto_ts_contract` in
    `crates/verter_protocol/tests/cases/`;
  - the submission port: the `engine_ports_*` compile-fail fixtures in
    `crates/verter_session/tests/cases/compile-fail/`;
  - carrier identity: `crates/verter_language/tests/cases/`
    (`parse_identity`, `registered_authorities`).

  New or extended tests belong to UAO0 and to the owners named per case.
- **AC3 — incremental equivalence: not applicable.** No cache,
  cancellation, stale-publication or partial-result authority is touched,
  and no production byte changes. `DR21`–`DR25`, `DR29`, `DR30` and the
  cases `DC05`, `DC06` and `DC15` bind the later proof to COX0 (`COX0-AC3`)
  and UAO0 (`UAO0-AC-R2`).
- **AC4 — bounded work: not applicable.** No hot path changes. `DR26`–`DR28`
  and the cases `DC01`, `DC02`, `DC10` and `DC13` fix the zero-work counts
  that COX0 (`COX0-AC4`) and UAO0 (`UAO0-AC-R2`) prove. The `Z01` baseline
  stays UAK0's, confirmed by PER0E (`PER0E-AC1`).
