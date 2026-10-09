# Canonical-symbol provenance and role activation

This decision defines how Verter proves that a source site plays a framework
role, such as "this call is Vue's `defineComponent`" or "this binding is
Svelte's `createEventDispatcher`". A role is proven from a canonical package
export, reached through a recorded chain of authored bindings and module hops,
and never from an identifier or import-specifier spelling. Producing the proof
consults no TypeScript program, no type resolver and no TypeInfo query, and
executes no package code. The proof is `RoleEvidence`. It is the role input
that DEM0's pre-projection `SourceActivationPlan` (DEM0 `DR07`) and
post-snapshot `SemanticClaimPlan` (DEM0 `DR11`) read.

Today, framework-shaped host/session registries and untagged public
boundaries own role recognition: each layer keeps its own spelling test. The
shared analysis classifies Vue APIs by import-source strings, the IDE
projection keeps four copies of a `"vue"` import-alias collector, the
type engine compares a route's terminal import source with `"vue"`, and the
Svelte script-fact seam derives a package name from the specifier text. The
final and sole owner is the typed immutable universal catalog, which holds the
role rows, and the demand-selected kernel service that produces
`RoleEvidence`.

It describes the repository at `docs(arch): record the parser decision,
ownership, reuse and lineage (#812)`, 2026-10-09. It follows the docs-only
rule in [README.md](README.md): it changes no production route and adds no
check. It reads the [identities](identities.md), [catalog](catalog.md),
[configuration](configuration.md), [parser](parser-ownership.md) and
[demand-activation](demand-activation.md) decisions, and does not re-own
anything they, the [authority inventory](authority-inventory.md) or the
[constitution](constitution.md) assign.

## Machine-readable products

The reviewed contract data lives in `tests/kernel/EAK0/products/`:

| File | Holds |
| ---- | ----- |
| `export-activation-inventory.v1.json` | Subblocks, role-registry row kinds, contract rules `ER01`–`ER30`, outcomes `EAK-O01`–`EAK-O10`, consumers `EAK-C01`–`EAK-C09`, displaced routes `EAK-D01`–`EAK-D14`, the DEM0, UAK0, VID0, CAT0 and PAR0 rows it references, charted consumers, boundaries that are not displaced routes, coverage of each deletion category, empty populations, findings and transferred obligations |
| `export-activation-case-table.v1.json` | Cases `EC01`–`EC30`: input, required and forbidden outcome, the rules each case exercises, existing evidence, and the node whose test makes it executable |

Every `successorPath` starts at EAK0 and follows successor edges in the
controller-owned plan. TIF1, COX0 and CPF1 are not successors of EAK0, so
their rows are referenced, never assigned here.

## Vocabulary

| Term | Meaning |
| ---- | ------- |
| `RoleId` | A role, qualified by its profile: `(FrameworkProfileId, RoleKey)`, or `(ProjectProfileId, RoleKey)` for a project-profile role. `RoleKey` is a closed per-profile vocabulary. |
| `CanonicalExport` | `(PackageName, ExportSubpath, ExportName, Space)`, where `Space` is `Value` or `Type`. The package name comes from the resolved package manifest, never from specifier bytes. |
| `ProfileIntrinsic` | A name the exact release's compiler reserves inside one region role, such as Vue `<script setup>` macros or Svelte runes. It is not a package export. |
| role site | A call or reference whose head is an identifier or a static member path, inside a unit a demand reaches. |
| link | One step of the chain from a role site to its terminal: a local binding, a namespace member, a destructured property, a `const` alias, a module re-export hop or a project-profile contribution. |
| `RoleEvidence` | The closed outcome for one role site: `Proven`, `NotRole`, `Unproven`, `Ambiguous` or a degraded stop. |

## Contract rules

### Canonical package/export role registry (subblock 1)

- **ER01.** Role rows are catalog data. Each `T03` semantic-profile row
  (CAT0) carries its role rows, and each `T04` project-profile row carries
  its project roles. Adding a role is a data change (CAT0 `CR13`). Rows are
  ordered by the canonical encoding of `RoleId` (CAT0 `CR05`) and are part of
  the `CatalogSnapshot` identity (VID0 `I09`). There is no role table beside
  the snapshot.
- **ER02.** A role row declares exactly one origin.
  `PackageExport { entries }` lists every admitted `CanonicalExport` of the
  exact release explicitly, for example `vue` and `@vue/runtime-core` for
  `defineComponent`. There is no prefix, glob or package family such as
  `vue/*` or `@vue/*`. `ProfileIntrinsic { region role, name, scope law }`
  names the reserved name, the region role it lives in (CAT0 `T01`, `T05`)
  and the release's scope law (`Reserved` or `UnshadowedFree`).
- **ER03.** A role row declares its phase. A `Projection` role is decidable
  from the link forms in `ER05`–`ER09` without type information. A
  `PostSnapshot` role needs the shared type resolver, as `Ref` or `Snippet`
  type wrappers do. A `PostSnapshot` role never activates projection and
  feeds DEM0 `P3` only. A role that genuinely needs post-snapshot meaning is
  never promoted to `Projection`. It stays out of projection activation (the
  charter's rescope clause).
- **ER04.** A profile intrinsic is proven only inside its declared region
  role, under its row's scope law, for the release the activation record
  admits. An import of the same name from a package
  (`import { defineProps } from 'vue'`) is not intrinsic evidence, and the
  same call outside the region is `NotRole { OutsideRegion }`.

### Link capture (subblock 2)

- **ER05.** Captured link forms:
  - a named import, including a renamed one (`import { defineComponent as dc } from 'vue'`);
  - a namespace import plus a static member: a dot member, or a computed
    member whose key is a string literal (`V.defineComponent`,
    `V['defineComponent']`);
  - `const` object destructuring of a namespace binding, where each
    property key is an identifier or a string literal
    (`const { defineComponent: make } = V`);
  - an immutable `const` alias whose initializer is a link
    (`const define = dc`, `const define = V.defineComponent`). Parentheses
    and the erasable TypeScript wrappers (`as`, `satisfies`, non-null `!`,
    angle-bracket assertion) keep the runtime identity, so they are
    transparent;
  - a local barrel: a workspace module's `export { x as y } from`,
    `export *`, `export * as ns`, or an `import` followed by
    `export { … }`.
- **ER06.** Not captured. Each of these ends the chain with the `NotRole`
  cause shown:
  - dynamic `import()`, `require`, `module.exports` and CommonJS interop:
    `Unsupported`;
  - a default import of a role export the release does not declare as
    default: `NotCanonical`;
  - a free reference with no binding: `Unbound`, unless `ER09` binds it
    through a project-profile contribution;
  - optional-chained members: `Unsupported`;
  - non-literal computed keys and rest elements: `Unsupported`.
- **ER07.** A type-only link (`import type`, `import { type X }`,
  `export type`) carries only `Type`-space roles. A `Value`-space role
  reached through a type-only link is `NotRole { TypeOnlyLink }`.
- **ER08.** Chains are followed one module hop at a time. Each hop reads
  two things only: the module-resolution answer for the hop's specifier (the
  shared import-route authority today, the PM resolution proof once it
  exists) and the hop module's parse-domain export inventory (the
  `IndexedReady` export table, PAR0's parse once per identity). A hop never
  reads TypeScript, the shared type resolver
  (`ProjectSemanticDispatch`), TypeInfo or a package's declaration bodies.
  It also never rescans raw source.
- **ER09.** The terminal is canonical only when the hop resolves into a
  package-backed module whose manifest name and export subpath equal a
  role-row entry. Package-backed means the workspace classifier
  (`workspace_is_package_backed`) says so. A specifier that a `paths` or
  alias mapping sends to a workspace file is workspace-owned, so it is
  `NotRole { NotCanonical }` even when its bytes read `vue`. A project-profile
  contribution hop (a captured auto-import of `ProjectProfileId`) is a link
  only when the profile's generated facts declare it, and it records that
  contribution's identity.

### Shadowing, mutation, wrapper and conditional failure (subblock 3)

- **ER10.** Shadowing. The binding that lexical scoping resolves for the
  site's head is the only first link. Scoping covers blocks, parameters,
  `catch`, class and function names, and hoisting. An import of the same
  name elsewhere in the file is irrelevant. A nearer non-link binding gives
  `NotRole { Shadowed }`.
- **ER11.** Mutation. Only import bindings and `const` bindings are links.
  A `let`, `var`, parameter or object property is never a link, even when it
  is never reassigned: `NotRole { Mutable }`. A local function or class
  declaration of the same name is a userland definition:
  `NotRole { LocalDeclaration }`. A re-export of a mutable workspace binding
  is `Mutable` too.
- **ER12.** Wrappers. A call, `.bind`, `new`, spread, or object or array
  literal around a link gives `NotRole { Wrapped }`. This covers
  `const api = { dc: defineComponent }` followed by `api.dc(…)`.
- **ER13.** Conditionals. `?:`, `&&`, `||`, `??`, a destructuring or
  parameter default, and assignment under control flow give
  `NotRole { Conditional }`.
- **ER14.** A hop whose specifier does not resolve, or whose package has no
  proven installed release, gives `Unproven { needs }`. It is never cached as
  a warm negative and never replaced by a name guess. When resolution later
  proves the package, a new evidence identity is minted (DEM0 `DR24`).
- **ER15.** Barrel ambiguity. Two `export *` providers that supply the name
  with different terminals give `Ambiguous { candidates }`, and no role is
  proven. Two paths to the same terminal `CanonicalExport` are not
  ambiguous: the recorded chain is the least by canonical encoding.
- **ER16.** A hop that revisits a `(module, export name)` pair gives
  `NotRole { Cycle }`. Exhausting the hop bound of the request's
  `BudgetProfile` gives `BudgetExceeded`, which is `ReturnOnly`.
- **ER17.** The outcome set is closed: `Proven`, `NotRole { cause }`,
  `Unproven { needs }`, `Ambiguous { candidates }`, `BudgetExceeded` and
  `Cancelled`. Only `Proven` activates a role. `Unproven`, `Ambiguous`,
  `BudgetExceeded` and `Cancelled` are `ReturnOnly` (DEM0 `DR30`). `NotRole`
  is definitive and may be cached only when every read in its read set
  completed.

### Package-resolution and read-set provenance (subblock 4)

- **ER18.** `RoleEvidence::Proven` carries:
  - `site`: `(SourceUnitId, SourceRevision, authored range)`;
  - `role`: the `RoleId`;
  - `chain`: hops in authored-to-terminal order;
  - `terminal`: the `CanonicalExport`;
  - `package`: package name plus the `ReleaseId` the activation record
    admits;
  - `read_set`.

  Hop kinds are `LocalImport`, `NamespaceMember`, `Destructure`,
  `ConstAlias`, `ReExport { module, specifier bytes, export name }` and
  `ProjectContribution { ProjectProfileId, contribution identity }`.
- **ER19.** The read set lists, in canonical encoding order:
  - the site unit's parse-domain facts identity;
  - every hop module's `(SourceUnitId, ContentId)` export inventory;
  - every module-resolution answer consulted, including negative probes;
  - the manifest identity of each package touched (name and export-map
    digest) and its installed release;
  - the role row's identity inside `CatalogSnapshot`;
  - any project-profile contribution identity.

  Evidence identity is the canonical tagged encoding of `(site, role,
  outcome, read-set identity)` (VID0 `R12`, `R13`). It carries no path
  string and no backend or process identity (VID0 `R14`).
- **ER20.** Invalidation follows the read set exactly. An edit to a member
  invalidates exactly the evidence that read it, and an edit to a
  non-member invalidates nothing. Edit-then-revert yields the same evidence
  identity, and incremental evidence equals fresh evidence. A changed
  evidence identity reaches DEM0 stage one only as a new stage-one plan,
  never as an in-place edit of the current generation (DEM0 `DR12`,
  `DR13`).
- **ER21.** Query keys stay content-free, following the repository's
  query-identity rule. Validity is the read set, revalidated on every warm
  hit. Work admitted under a superseded epoch never publishes (DEM0 `DR25`).
- **ER22.** Ordering is deterministic. A unit's evidence is ordered by site
  range, then `RoleId` encoding. Chains run authored to terminal. Ambiguity
  candidates follow canonical encoding. Registration, discovery and
  completion order never decide.

### Activation evidence for verticals (subblock 5)

- **ER23.** `Projection`-phase evidence is DEM0 `P2`'s role input
  (`DR07`), and `PostSnapshot` evidence is `P3`'s (`DR11`). A vertical reads
  evidence by `RoleId`. It never re-derives a role from an identifier,
  callee name, import-source string, file extension or text search.
- **ER24.** A projection region opens only from `Proven` evidence of a
  `Projection` role whose row declares that region. One example is the
  `template` option of a proven Vue `defineComponent` call, which opens an
  embedded template region. Region geometry and codecs are EMB0's.
- **ER25.** No oracle. Evidence production calls no TypeScript provider and
  no TCM3 stage, and executes no package. `PostSnapshot` roles use the
  shared type resolver only inside `P3`, through the exact-route authority
  that `resolve_authored_reference_route` already implements.
- **ER26.** Under DEM0's `Explain` purpose (`DR17`), consumers surface the
  chain of `Proven` evidence and the cause of every other outcome. Audit
  capture is optional detail. Required evidence state survives capture off.
- **ER27.** Work stays bounded. Evidence is computed only for role sites a
  demand reaches, and only when the head binds to an import, a namespace or
  a captured contribution. Each hop module's export inventory is read once
  per `(SourceUnitId, ContentId)`. A unit with no such binding does no
  evidence work beyond its existing import inventory, which keeps DEM0
  `DR26` and `DR27` intact.
- **ER28.** A `TypeInfoRequest` never carries a role name, a package
  spelling or a specifier as a selector. Framework surfaces are addressed
  through the claims of `P3`. TIF0 owns the request vocabulary.

### Positive and same-spelling negative corpus (subblock 6)

- **ER29.** The corpus is `export-activation-case-table.v1.json`. Each
  positive case has at least one same-spelling negative that differs only
  in the link that must fail. EAK1 builds and runs it (`EAK1-AC-R1`) in its
  existing test lanes. EAK0 ships no test.
- **ER30.** Runtime compile backends are not role-evidence consumers for
  intrinsics. They keep the official compiler's own recognition, governed
  by Compiled-Output Conformance (`EAK-B01`). A backend that needs a
  `PackageExport` role reads `RoleEvidence` like any other consumer.

## Outcomes and owners

| Outcome | Owner | Receiving acceptance |
| ------- | ----- | -------------------- |
| `EAK-O01` role rows in `T03` and `T04`, with origin and phase (`ER01`–`ER04`) | EAK1 | `EAK1-AC2` |
| `EAK-O02` link capture (`ER05`–`ER09`) | EAK1 | `EAK1-AC2` |
| `EAK-O03` failure taxonomy and closed outcome set (`ER10`–`ER17`) | EAK1 | `EAK1-AC1` |
| `EAK-O04` `RoleEvidence` record, read set and identity (`ER18`, `ER19`, `ER22`) | EAK1 | `EAK1-AC2` |
| `EAK-O05` read-set invalidation, revert equality and no degraded warming (`ER20`, `ER21`) | EAK1 | `EAK1-AC3` |
| `EAK-O06` evidence exposure to verticals, `Explain` and the request boundary (`ER23`, `ER24`, `ER26`, `ER28`) | EAK1 | `EAK1-AC2` |
| `EAK-O07` no oracle and bounded, demand-scoped work (`ER25`, `ER27`) | EAK1 | `EAK1-AC4` |
| `EAK-O08` positive and same-spelling negative corpus, built and run (`ER29`, `ER30`) | EAK1 | `EAK1-AC-R1` |
| `EAK-O09` project-profile auto-import contributions as `ProjectContribution` links (`ER09`) | NUX0 | `NUX0-AC1` |
| `EAK-O10` executable ownership validator and negative controls for this decision | UAO0 | `UAO0-AC-R1` |

EAK1 reaches EAK0 through EMB0. It is the first production node behind
EAK0 that implements role evidence, and its charter already deletes the
three named categories (`EAK1-AC1`). DEM0's `DEM-O09` names EAK1 as the
producer of the role evidence `P2` and `P3` read; `EAK-O01`–`EAK-O07`
specify that outcome.

## Displaced routes recorded here

All of these share one population: the duplicated macro and import
recognizers that the reconciled contract deletes after consumers migrate.
Each has one production-capable deletion owner. The inventory lists the
exact symbols and paths of each route.

| Route | Unit | Disposition | Deletion owner |
| ----- | ---- | ----------- | -------------- |
| `EAK-D01` | Shared Vue API classification by specifier spelling: `is_vue_source`, `classify_vue_api` and the `AnalyzedImportBinding.vue_api` it fills, in `verter_semantic` | replace with `PackageExport` evidence; consumers keep reading a typed classification minted from evidence | EAK1 (`EAK1-AC1`) |
| `EAK-D02` | `detect_vue_api_call` in `verter_parser`'s Vue script usage walk: bare callee bytes, feeding the provide/inject, lifecycle and watcher usage facts of `verter_semantic` `file_usage` | replace with evidence | EAK1 (`EAK1-AC1`) |
| `EAK-D03` | The macro-usage visitor's `vue_value_imports`, built from `imp.source == "vue"` in `build_script_analysis` | replace with evidence | EAK1 (`EAK1-AC1`) |
| `EAK-D04` | Four copies of a `"vue"` import-alias collector in the IDE projection: `options_api` and `binding_views` `vue_runtime_imports`, `script_setup` `vue_runtime_macro_imports` (local name only), `type_constructs` `proven_vue_async_component_bindings` | replace with one evidence read | EAK1 (`EAK1-AC1`) |
| `EAK-D05` | Free-name acceptance in the IDE projection: `is_vue_wrapper` and `factory_export` treat an unbound `defineComponent`, `ref` and similar names as Vue's | reject: `NotRole { Unbound }` (`ER06`) | EAK1 (`EAK1-AC1`) |
| `EAK-D06` | Bare-callee Vue API recognizers in the IDE script path (`detect_use_attrs_calls`, `detect_gci_in_expr`, `record_ref_variable_call`, `maybe_record_use_template_ref_call`), and the extract-bare-text action's `find_or_extend_vue_import`, which finds an existing `computed`/`unref` import by local name | replace with evidence | EAK1 (`EAK1-AC1`) |
| `EAK-D07` | `.tsc` stub `defineComponent` import decision by local name and `"vue"` source (`generate_options_api_stub`), and `useAttrs` detection by bare name (`detect_use_attrs_type_arg_tsc`) | replace with evidence | EAK1 (`EAK1-AC1`) |
| `EAK-D08` | Duplicated intrinsic-macro vocabularies in Verter-owned analysis and IDE paths: `classify_macro`, `COMPILER_MACRO_NAMES`, `MACRO_NAMES` in `script_setup` and `script_recover` (raw-token match), `ScriptPropFacts::note_value`'s bare `call_name` | replace with the `ProfileIntrinsic` rows (`ER04`); the scope law is row data | EAK1 (`EAK1-AC1`) |
| `EAK-D09` | Type-engine role gate by spelling: `wrapper_candidate_for_route` compares `terminal_import_source` with `"vue"` and mints `package: "vue"`, and `wrapper_role_for_vue_export` holds a private vocabulary | replace the spelling gate with the role row and manifest package identity; keep the exact-route authority | EAK1 (`EAK1-AC1`) |
| `EAK-D10` | Svelte package identity from specifier bytes: `resolved_package_for_import` and `bare_specifier_package_name` in `framework/script_facts.rs`, plus the `Snippet` and `createEventDispatcher` imported-name capture they validate | replace with `ER09` manifest identity and Svelte role rows | EAK1 (`EAK1-AC1`) |
| `EAK-D11` | Rule-local Vue role spelling in markup and deprecation rules: `no_deprecated_delete_set` (local `set`/`delete` from `"vue"`) and `prefer_script_attrs` (text search for `useAttrs`, with its action twin) | replace with evidence when the rule moves to the lint service | LVU1 (`LVU1-AC1`) |
| `EAK-D12` | Rule-local Vue role spelling in SFC rules: `no_import_compiler_macros` (`source == "vue"`), `no_reserved_component_names` (`is_allowed_source` prefix allow-list) | replace with evidence | LVU2-SFC (`LVU2-SFC-AC1`) |
| `EAK-D13` | Store-role spelling: `classify_store_api`, `is_store_composable_call` (`use*Store` plus a `/store` path substring), `no_unused_store_import` `is_store_source` | replace with `PackageExport` rows for the store packages | LVU2-STORES (`LVU2-STORES-AC1`) |
| `EAK-D14` | Built-in Vue component spelling in `no_undef_components` (`BUILTIN_COMPONENTS`), separate from the reserved-name list | replace with role rows for the release's built-in components | LVU1-BINDINGS (`LVU1-BINDINGS-AC1`) |

The LVU groups' charters already forbid "activation by identifier spelling
when binding identity exists". Each deletes its rule-local route when that
rule moves onto the lint service. Ownership of `EAK-D01`–`EAK-D10` is the
default of open operator question `eak0-recognizer-deletion-owner`. A
ruling that adds a dedicated migration node moves those rows only.

EAK0 references these routes owned elsewhere:

| Route | Owner | Concern |
| ----- | ----- | ------- |
| DEM0 `DEM-D04` | EAK1 | `should_ignore_external_macro_type` compares `import_source == "vue"`; it stays DEM0's row |
| DEM0 `DEM-D05` | NUX0 | Nuxt server/client detection by file-name suffix |
| UAK0 `D07`, `D08` | CPF1 | One framework per file by extension, and the conflated `FileLanguage` |
| UAK0 `D10`, `D11` | COX0 | Per-framework LSP branches and MCP `is_vue()` gates |
| UAK0 `D12` | TIF1 | Component-meta resolver authority, including the `.vue` path-suffix carrier check (DEM0 `DEM-F03`) |
| VID0 `V-D02` | TIF1 | The open `framework_adapter_id` string |
| CAT0 `CAT-D06` | COX0 | `ActiveProviderIndex` and its import-specifier gate (DEM0 `DEM-F02`) |

Deletion-category coverage:

| Category | Recorded here | Referenced |
| -------- | ------------- | ---------- |
| central framework switch | `EAK-D01`–`EAK-D14` | `DEM-D04`, `D07`, `D10`, `D11`, `CAT-D06` |
| untagged coordinate/public identity | none: role evidence adds no public identity route, and every evidence identity is tagged (`ER19`) | `D08`, `V-D02` |
| duplicate component information authority | none: the recognizers above feed existing authorities and are not authorities themselves | `D12`, `DEM-D05` |
| duplicated macro/import recognizer (this decision's own population) | the same `EAK-D01`–`EAK-D14`: each route has one category, central framework switch, as DEM0 categorised `DEM-D04` | `DEM-D04` |

## Consumers

| Consumer | Reads | Owner |
| -------- | ----- | ----- |
| `EAK-C01` Vue IDE projection: options API, binding views, script setup, props facts | `Projection` evidence, `ProfileIntrinsic` rows | EAK1 (`EAK1-AC2`) |
| `EAK-C02` Vue embedded-template region activation from a proven `defineComponent` call | `Projection` evidence, `ER24` | EAK1 (`EAK1-AC2`) |
| `EAK-C03` Shared script analysis consumers of the typed Vue API classification: MCP tools, LSP hover/completion/organize-imports/document symbols, file-usage provide/inject facts | evidence-minted classification | EAK1 (`EAK1-AC2`) |
| `EAK-C04` Template-class and binding-return wrapper facts | `PostSnapshot` evidence | EAK1 (`EAK1-AC2`) |
| `EAK-C05` Svelte script-fact resolved validation (`Snippet`, dispatcher) | `Projection` and `PostSnapshot` evidence | EAK1 (`EAK1-AC2`) |
| `EAK-C06` Vue Custom Element producer/consumer retrofit | `Projection` evidence | VCE0 (`VCE0-AC2`) |
| `EAK-C07` Vue lint rules on the lint service | evidence by `RoleId` | LVU1 (`LVU1-AC1`) |
| `EAK-C08` Nuxt auto-import contributions | `ProjectContribution` links | NUX0 (`NUX0-AC1`) |
| `EAK-C09` Embedded-region activation by binding-resolved tags and sinks | evidence by `RoleId` | INT5 (`INT5-AC5`) |

DEM0 already owns the stage consumers: `P2` and `P3` read evidence
(`DEM-O09`, `DR07`, `DR11`, owner COX0). The audit runtime receives the plan
records (`DEM-C07`). EAK0 references them and does not re-own them.

Sixty-four later family charters already bind themselves with the line "Role
identification by canonical symbol (EAK0 provenance through the owner
facts), never by identifier spelling": `CTX1-*`, `EFF3-*`, `FWD1-*`,
`HYD1-*`, `RND1-*`, `TRN1-*` and `CPD1-*`. Each is a successor of EAK0. The
inventory lists them under `chartedConsumers`. Their own charters own the
obligation, so EAK0 assigns them nothing.

## Boundaries that are not displaced routes

- **`EAK-B01` runtime compile backends.** The Vue VDOM/Vapor and Svelte
  runtime compilers recognize intrinsics by spelling and scope, as the
  official compilers do: the `rune_scan`, `needs_context`, `expr` and
  `reactive_analysis` rune helpers, `store_subscriptions`' local-name
  `derived` rule, the `verter_parser` macro byte tables and the
  `classify_call_expression` binding-metadata classifier, the Vue compile
  path's `check_macro_call` (the official `checkInvalidScopeReference`) and
  `collect_call_section`, and `client_surface`'s dispatcher locals. Compiled-Output Conformance governs
  their output. Role evidence does not displace them (`ER30`). Their
  duplicated rune vocabularies are finding `EAK-F05`.
- **`EAK-B02` specifier as the rule subject.** `prefer_import_from_vue` and
  its `replace_content` fix inspect the import specifier because the
  specifier is what the rule reports. They recognize no role. Their
  disagreeing package lists are finding `EAK-F06`.
- **`EAK-B03` template instance globals.** `$props`, `$emit` and `$slots`
  in a template are members of the component instance surface. They are not
  package exports, so LSP definition mapping them to macros is instance
  surface, not role activation.
- **`EAK-B04` package detection.** `vue_assets` and `svelte_assets` read the
  manifest name to locate a framework's assets. That is package detection,
  which FWA1's activation record owns, not role activation.

## Findings recorded for the receiving owners

- **Unbound names count as Vue's** (`EAK-F01`, EAK1). `is_vue_wrapper` and
  `factory_export` accept a free `defineComponent` or `ref`. `ER06` makes
  that `NotRole { Unbound }`. This is `EAK-D05`.
- **Alias semantics disagree across copies** (`EAK-F02`, EAK1).
  `vue_runtime_macro_imports` keys on the local name, so
  `import { defineProps as dp }` is not followed. `generate_options_api_stub`
  keys on the local name too, so `import { x as defineComponent }` is
  accepted while an aliased real import still gets a duplicate inject. Three
  other collectors key on the imported name.
- **Macro recognition does not check shadowing** (`EAK-F03`, EAK1).
  `classify_macro` and `ScriptPropFacts::note_value` match the callee
  spelling without consulting a binding. `ER04` makes the scope law row
  data, checked against the release.
- **The Svelte package name comes from specifier bytes** (`EAK-F04`, EAK1).
  `bare_specifier_package_name` takes the leading specifier segment once the
  resolved file is package-backed. A `paths` mapping of `svelte` onto
  another package's file would therefore claim `svelte`. `ER09` takes the
  name from the resolved manifest.
- **Rune vocabularies are duplicated** (`EAK-F05`, EAK1). `$bindable`,
  `$inspect`, `$props` and the rune-name lists each have two to four
  spellings across the Svelte runtime and IDE paths. The Svelte IDE uses
  text prefilters (`contains("$props")`, `contains("$host")`). The IDE
  copies are `EAK-D08`'s population. The runtime copies stay `EAK-B01`.
- **Internal-package lists disagree** (`EAK-F06`, LVU2-SFC). The
  `prefer_import_from_vue` diagnostic lists five `@vue/*` packages, and the
  `replace_content` fix handles four, missing `@vue/composition-api`.
- **The exact-route authority already exists** (`EAK-F07`, EAK1).
  `resolve_authored_reference_route` composes authored aliases, imports,
  re-exports and local alias hops into a `ResolvedReferenceRoute` with an
  `exactness` and a recorded route. It is the `PostSnapshot` half of
  `ER25`. The `Projection` half (`ER08`) must reach the same answer without
  the type resolver; EAK1 proves the two agree on `PostSnapshot` cases.

## Empty populations

- **No role rows or role identities.** No `RoleId`, `RoleEvidence`,
  `CanonicalExport` or role table exists under `crates/`. `T03` rows do not
  exist yet (CPF1).
- **No projection-time role evidence.** Every projection-time recognizer is
  a spelling test. The only provenance-carrying role route is the
  post-snapshot `ResolvedReferenceRoute` (`EAK-F07`).
- **No barrel-following at projection time.** No projection-time
  recognizer follows a re-export, a namespace destructuring or a `const`
  alias chain. `proven_vue_async_component_bindings` alone follows a
  namespace member.
- **No project-profile role.** No auto-import contribution reaches role
  recognition; `T04` has no rows (CAT0).
- **No ambiguity outcome.** No recognizer can report an ambiguous barrel.

## Acceptance evidence

Evidence selection: the change adds contract text and data only, so existing
coverage and bounded inspection discriminate it. The diff adds no test.

- **AC1 — ownership contract.** The inventory binds every rule, outcome,
  consumer and displaced route to one existing plan node, a successor path
  from EAK0 and a receiving acceptance ID. DEM0, UAK0, VID0 and CAT0 rows
  are referenced, not re-owned. Each later family charter that already binds
  itself to EAK0 provenance is listed, not assigned. The executable validator
  is UAO0's (`UAO0-AC-R1`). The corpus is EAK1's (`EAK1-AC-R1`).
- **AC2 — positive contract.** Existing coverage pins exact identity,
  provenance and fail-closed behaviour for today's one canonical route and
  the spelling routes it replaces. In `verter_session` `src/tests/host_manage`:
  - `workspace.rs`:
    `return_wrapper_routes_follow_renamed_imports_local_aliases_and_barrels`,
    `template_class_wrapper_routes_follow_import_then_export_and_local_alias_barrels`
    and `return_wrapper_role_rejects_local_and_foreign_package_fakes`;
  - `compilation.rs`:
    `template_class_facts_accept_exact_vue_wrappers_and_reject_local_fakes`
    and
    `template_class_wrapper_artifact_binds_each_duplicate_terminal_route_exactly`;
  - `resolution.rs`:
    `component_meta_binding_role_rejects_local_and_foreign_wrapper_fakes`;
  - `general.rs`:
    `return_wrapper_roles_cover_the_exact_vue_vocabulary_structurally`.

  For Svelte, `userland_snippet_lookalike_is_not_classified_snippet_typed`
  and `svelte_synth_is_identical_with_real_vs_fake_svelte_package` (in
  `svelte_vertical_tests.rs`), and
  `userland_snippet_lookalike_is_not_published_as_a_slot` (in
  `svelte_exec_tests.rs`). For the spelling routes,
  `aliased_vue_import_classified_by_imported_name` in `verter_semantic`, and
  `stp13_vue_import_alias_still_unwraps_by_binding` in `verter_compiler`.
  For the carrier and request boundaries, `verter_language`
  `tests/cases/` (`parse_identity`, `registered_authorities`) and
  `verter_protocol` `tests/cases/` (`typeinfo_proto_roundtrip`,
  `typeinfo_proto_ts_contract`). New or extended tests belong to EAK1 and to
  the owners named per case.
- **AC3 — incremental equivalence: not applicable.** No cache, cancellation,
  stale-publication or partial-result authority is touched, and no
  production byte changes. `ER17` and `ER20`–`ER22` bind the later proof to
  EAK1: `EC22` states the incremental-invalidation acceptance
  (`EAK1-AC-R1`), and `EC23`–`EC25` the degraded-outcome cases
  (`EAK1-AC3`).
  `return_wrapper_role_degrades_typed_and_is_never_warmed` (in
  `caching.rs`) already pins the no-warm half for the post-snapshot route.
- **AC4 — bounded work: not applicable.** No hot path changes. `ER27` and
  the cases `EC26` and `EC27` fix the zero-work and read-once counts that
  EAK1 (`EAK1-AC4`) proves.
  `template_class_requested_subject_does_not_read_unrelated_cold_wrapper_import`
  and `return_wrapper_demand_does_not_read_unrelated_cold_wrapper_import`
  (in `resolution.rs`) pin today's demand-scoped reads.
