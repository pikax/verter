# Carrier frontend / compiler-backend separation

This decision proves that the compiler-shaped carrier abstraction splits into a
carrier frontend that every carrier has and an optional compiler backend that
only compile-capable carriers register, without weakening current compilation
or tooling. Today, framework-shaped host/session registries and untagged public
boundaries own this split. The final and sole owner is the typed immutable
universal catalog and the demand-selected kernel services.

It describes the repository at `docs(arch): ratify orthogonal identities and
the exact-release law (#802)`, 2026-10-09. It follows the docs-only rule in
[README.md](README.md): it changes no production route and adds no check. It
builds on the [authority inventory](authority-inventory.md), the
[constitution](constitution.md) and the [identities](identities.md) decision,
and does not re-own anything they assign.

## Machine-readable products

The reviewed contract data lives in `tests/kernel/CPF0/products/`:

| File | Holds |
| ---- | ----- |
| `carrier-split-inventory.v1.json` | Every carrier-compiler method `M..`, every product `P..` with its frontend/backend class, the contracts `T01`–`T03`, capability rows `CR..`, outcomes `F-O..`, consumers `F-C..`, displaced routes `F-D..`, the referenced UAK0/UAK1/VID0 routes, coverage of each deletion category, empty populations and transferred obligations |
| `carrier-split-case-table.v1.json` | Type-level proof cases `PC..` (Vue, Svelte, tooling-only HTML and Astro stubs) and the work-counter specification `WC..` that CPF1 compiles and reports |

Every `successorPath` starts at CPF0 and follows predecessor edges in the
controller-owned plan. UAK0's `B..`/`O..`/`C..`/`S..`/`D..` rows, UAK1's
`U..`/`K..` rows and VID0's `I..`/`V-..` rows keep their owners; CPF0 refines or
references them by id.

## Method

1. Enumerated every associated method of the two compiler-shaped carrier
   structs (`VueCarrierCompiler`, `SvelteCarrierCompiler`) and of the closed
   `KnownRegisteredCompiler` enum, with every production and test caller.
   `render_admitted_svelte_ide`, `svelte_ide_only_request` and
   `svelte_carrier_bundle` are free functions after `impl SvelteCarrierCompiler`
   (`M21`–`M23`), not associated methods. Each has its own row, callers, owner
   and route.
2. Enumerated the free functions and product types in `vue_bridge.rs`,
   `svelte/carrier.rs`, `carrier_compiler.rs`, `registered_carrier_projection.rs`,
   `catalog.rs` and `standalone.rs`, and the five capability traits with their
   Vue/Svelte implementations. Followed each to its consumers in `verter_session`,
   `verter_lsp`, the `parse_corpus_probe` binary, `StandaloneCompiler`,
   `verter_tsc`, NAPI, WASM, FFI and MCP. Paths follow producer→consumer edges,
   not name matches. The compile-request envelope the transports and `verter-tsc`
   name is `P17` (`F-C12`, `F-C14`): CMP0 owns its contract and CMP1 its
   production migration. `StandaloneCompiler::compile`
   and `prepare` call `parse_sfc` / `parse_svelte` directly (`F-C13`, CPF1).
3. Classified each product as frontend, semantic, projection, compiler backend
   or residue. Classified each module edge against UAK1's layers: the frontend
   half is `L3`, which may not import the optional compiler backends (`LC`).
4. Chose each owner from the descendants whose charter allows production work
   and names that route: CPF1 for the split and the Vue/Svelte cutover, FWC1
   for the kernel crate edge, CMP1 for the compile-request consumers, UAI0 for
   the validator. A docs-only contract node is never an implementation owner;
   where one defines the boundary a row consumes, the row names it as
   `contractOwner` beside the production-capable `owner`.

## What exists at the described head

The combined `CarrierCompiler` trait is already gone. `capability.rs` defines
five separate traits: `CarrierFrontend`, `FrameworkSemanticAuthority`,
`ProjectionBackend`, `RuntimeCompilerBackend` and
`FrameworkHostIntegrationBackend`. `catalog.rs` freezes typed rows of each.
Absence is already type-level: a frontend-only or projection-only row has no
runtime accessor (compile-fail `frontend_only_has_no_runtime_accessor`,
`projection_only_has_no_runtime_accessor`).

The compiler shape survives in three places:

- **Two compiler-shaped structs.** `VueCarrierCompiler` and
  `SvelteCarrierCompiler` hold the carrier identity (`M01`, `M02`, `M11`,
  `M12`), the parse (`M03`, `M13`) and the typed downcasts (`M04`–`M06`,
  `M14`–`M17`). All five capabilities delegate to them. The Vue struct also
  keeps test-support `compile_ide` and `compile_bundle` shims (`M07`–`M09`).
- **Shared modules.** `vue_bridge.rs` and `svelte/carrier.rs` mix the frontend
  half (parse, carrier payload, artifact constructor, openers) with the backend
  half (bundle orchestration, runtime assembly, custom blocks, style
  continuations, the Vue audit compile).
- **Closed enums.** `KnownRegisteredCompiler`, `InstalledCarrierFrontend`,
  `InstalledHostIntegration` and `InstalledRuntimeBackend` (UAK0 `D02`). The
  semantic and projection catalogs already dispatch through function-pointer
  rows with no framework match. That shape is the precedent for the two new
  registries.

## Classification

| Class | Products | Present for |
| ----- | -------- | ----------- |
| frontend | `P01` parse artifact, `P02` registered geometry, `P03` parse diagnostics and reject, `P04` parse admission, `P05` grammar fact, `P06` carrier opener, `P07` script source type | every carrier |
| semantic | `P08` eval source, `P09` template facts | each semantic profile that registers one |
| projection | `P10` IDE companion, public API and declarations; `P16` generated-identifier spelling | each profile that registers one |
| compiler backend | `P11` runtime output and maps, `P12` compile admission and grants, `P13` bundle orchestration, `P14` compile artifact set, `P15` Vue audit compile, `P17` compile-request envelope (contract CMP0, migration CMP1) | compile-capable carriers only (`P17` is the request contract the transports name) |
| residue | `M07`–`M10`, `M18`–`M20` | nobody; deleted |

The optional backend alone owns compiler output bytes and maps: runtime
client/server code, runtime maps, runtime diagnostics and the compile artifact
set. IDE projection is a tooling product, not a compiler product. It is
optional, a runtime demand never requires it, and it never requires a backend.

## Contracts

### `T01` — `CarrierFrontend` (amended)

- Exactly one frontend row per carrier profile (VID0 `I03`).
- `parse` yields the neutral `UnregisteredFrameworkParseArtifact` from
  `verter_language`, or a typed `SyntaxReject` before any artifact exists.
- Registered geometry, parse diagnostics, the parse admission, the grammar fact
  and the typed carrier accessors belong to the frontend.
- The frontend's module closure imports no `LC` module: `compile`,
  `compile_transaction`, `assembly`, `standalone`, `svelte::runtime`,
  `style_planner`, `template::code_gen`, script codegen or `tsc`.
- Every other capability consumes the one admitted parse and never re-parses.

### `T02` — `CarrierCompilerBackend` (new grouping)

- Groups `RuntimeCompilerBackend` and `FrameworkHostIntegrationBackend` into
  one optional registration in `CarrierCompilerBackendRegistry`, over
  `CatalogSnapshot` rows (the UAK0 `D02` successor).
- Only compile-capable carriers register one. It owns `P11`–`P15`, including
  the Vue audit compile route. That route is a backend-contract product;
  audited compile reaches the backend only through the registry (`F-D07`).
- It consumes the frontend's admitted parse and never parses.
- Absence is an omitted registration, never a stub that returns `Unsupported`.

### `T03` — absence is a capability status

- Tooling demands (parse, geometry, diagnostics, eval source, template facts,
  IDE projection) are admitted and served with no backend row.
- A compile demand on a carrier with no backend reports that product as
  `UNSUPPORTED` and changes no other product. This is the status family the
  framework-surface executor already reports.
- "No compiler" is never an error diagnostic, refusal or failed request on a
  tooling path.

### Capability rows

| Row | Capability | Vue | Svelte | HTML stub | Astro stub |
| --- | ---------- | --- | ------ | --------- | ---------- |
| `CR01` | frontend (exactly one) | yes | yes | yes | yes |
| `CR02` | semantic | yes | yes | — | — |
| `CR03` | projection | yes | yes | — | yes |
| `CR04` | runtime | yes | yes | — | — |
| `CR05` | compile admission | yes | yes | — | — |
| `CR06` | compiler backend registration (implies `CR01`, `CR04`) | yes | yes | — | — |

Read by column, this table states "all carriers have a frontend; only
compile-capable carriers require a backend". CPF1 makes it mechanically
exhaustive.

## Migration map

| Today | Target | Owner |
| ----- | ------ | ----- |
| `VueCarrierCompiler`/`SvelteCarrierCompiler` identity | profile row identity (VID0 `I03`/`I05`) | CPF1 (`F-D02`) |
| struct `parse` (`M03`, `M13`) | body of the frontend's `parse`, in a frontend module | CPF1 (`F-D01`) |
| `compile::parse_sfc`, `svelte::parse_svelte`, `svelte::runtime::official_reject::deferred_parse_defects_excluding_css` | frontend-owned parse and parse-defect modules; `StandaloneCompiler::compile` and `prepare` consume that parse (`F-C13`) | CPF1 (`F-D01`) |
| typed downcasts (`M04`–`M06`, `M14`–`M17`) | frontend-owned typed accessors | CPF1 (`F-D02`) |
| `compile_ide`, `compile_bundle` shims and their absence guards | deleted; tests drive the typed backends | CPF1 (`F-D03`) |
| Vue template facts via `compile_from_parsed_legacy` with `TEMPLATE_DATA` | codegen-free extractor on the semantic row | CPF1 (`F-D04`) |
| IDE projection importing `template::code_gen` helpers and `standalone` | neutral helper module shared with runtime | CPF1 (`F-D05`) |
| `RuntimeCompileOutput.{tsx, template_data}` | each product returned by its own row | CPF1 (`F-D06`) |
| `compile_registered_vue_artifact` audit route | the backend registry | CPF1 (`F-D07`) |
| `FrameworkEpochId`/`HostEpochId` spellings, literal `"vue"` | profile ids; spellings are display only | CPF1 (`F-D08`) |
| closed enums and built-in catalogs | the two registries | CPF1 (UAK0 `D02`) |
| `verter_session` → `verter_compiler` edge; Vue/Svelte parser in the kernel | optional backend registry populated by the composition root | FWC1 (UAK1 `K01`, `K03`) |

All Vue and Svelte routes move in one CPF1 change. The structs, enums and shims
are deleted in that same change, and no alias of the combined shape survives.

## Compile-request consumers

`P17` is the compile-request envelope. CMP0's mutation boundary permits only
contract evidence, so it owns the envelope's contract (`contractOwner`, `CMP0-AC1`,
`CMP0-AC2`) and cannot receive a production migration. The migration of its
production consumers belongs to CMP1 (successor path CPF0 → CMP0 → CMP1,
`CMP1-AC1`). CMP1's compatibility closure makes the typed `CompileRequest`
and `CompilerPolicy` the sole production admission route, and that change is
what forces every envelope constructor to move.

| Row | Consumer paths | Contract | Implementation |
| --- | -------------- | -------- | -------------- |
| `P17` | the envelope (`compile_request`) | CMP0 (`CMP0-AC2`) | CMP1 (`CMP1-AC1`) |
| `F-C12` | `verter_napi` `host_compile_request.rs`, `compile_request_response.rs`, `lib.rs`; `verter_wasm` `lib.rs`; `verter_ffi` `convert/input.rs` | CMP0 (`CMP0-AC1`) | CMP1 (`CMP1-AC1`) |
| `F-C14` | `verter_tsc` `checker.rs`, `main.rs` | CMP0 (`CMP0-AC1`) | CMP1 (`CMP1-AC1`) |

**Receiving-charter amendment `RA01` (ratified 2026-10-09).** The architect
ruling `cpf0-p17-implementation-receiver` extends CMP1's production surfaces
and declared write scope to the seven consumer paths above, solely for the
envelope migration under `CMP1-AC1` and its in-module tests. It supersedes the
F10 scope restriction for those paths. The corresponding conflict domains
are `area:crates/verter_napi`, `area:crates/verter_wasm`,
`area:crates/verter_ffi` and `area:crates/verter_tsc`, in addition to CMP1's
existing compiler and semantic domains. Other sibling ownership is unchanged.

CMP1 migrates these consumers in the same cutover that removes mixed
request/policy admission. Its acceptance exercises the real NAPI, WASM, FFI
and `verter-tsc` request boundaries, preserving accepted products and typed
refusals. Structural rejection of the displaced route must also reject a
negative control attempting legacy admission through the same boundary.
The inventory records this obligation under `receivingCharterAmendments`;
the controller owns the receiving charter. This repair authorizes the later
migration and does not claim it is implemented. The parse cutover of `F-C13`
stays CPF1's.

## Displaced routes recorded here

| Route | Category | Unit | Owner |
| ----- | -------- | ---- | ----- |
| `F-D01` | central framework switch | Frontends import runtime-codegen modules (Vue parse in `compile/mod.rs`, also called from `standalone.rs`; Svelte parse defects in `svelte::runtime`) | CPF1 (`CPF1-AC1`) |
| `F-D02` | central framework switch | The two compiler-shaped carrier structs | CPF1 (`CPF1-AC1`) |
| `F-D03` | central framework switch | Test-support combined compile shims and their guards | CPF1 (`CPF1-AC1`) |
| `F-D04` | central framework switch | Vue template facts computed by the runtime compile entry | CPF1 (`CPF1-AC1`) |
| `F-D05` | central framework switch | IDE projection reaches runtime-codegen modules | CPF1 (`CPF1-AC1`) |
| `F-D06` | duplicate component information authority | The runtime bundle carries the IDE companion and template facts | CPF1 (`CPF1-AC1`) |
| `F-D07` | central framework switch | Vue-only audit compile route outside the registry | CPF1 (`CPF1-AC1`) |
| `F-D08` | untagged coordinate/public identity | String-spelled catalog keys and literal adapter spellings | CPF1 (`CPF1-AC1`) |

Each deletion category maps to recorded and referenced routes:

| Category | Recorded here | Referenced |
| -------- | ------------- | ---------- |
| central framework switch | `F-D01`–`F-D05`, `F-D07` | `D01`–`D03`, `D05` (CPF1); `K01`, `K03` (FWC1) |
| untagged coordinate/public identity | `F-D08` | `D08`, `V-D04` (CPF1) |
| duplicate component information authority | `F-D06` | `D12`, `D14` (TIF1) |

Empty populations at this head:

- **A combined trait or dynamic compiler registry.** None exists (UAK0).
- **Host or transport callers of a combined compile entry.** `verter_session`,
  `verter_lsp`, NAPI, WASM, FFI, MCP and unplugin have no production caller of
  `compile_bundle`, `compile_ide`, `compile_runtime`, `project_ide` or
  `CarrierFrontend::parse`. NAPI, WASM and FFI do name the compile-request
  envelope in production (`P17`, `F-C12`). MCP names none of those Rust
  symbols. unplugin names the JS `HostCompileRequest` wire only. `verter_lsp`
  uses only the generated-identifier spelling (`F-C10`). The
  `external-corpus` binary `parse_corpus_probe` is a compiler-bin consumer of
  `parse_registered_frontend` (`F-C11`), not a host or transport caller.
  `StandaloneCompiler::compile` and `prepare` re-parse through `parse_sfc` and
  `parse_svelte` and never enter `CarrierFrontend::parse` (`F-C13`, CPF1).
  `verter-tsc` `generate_all_tsx`, reached from `pub fn run`, drives that
  compile and names the compile-request envelope (`F-C14`, CMP1).
- **`Unsupported` compiler implementations.** None exists, and `T03` forbids
  one.

## Proof cases and work counters

The case table holds `PC01`–`PC08`:

- **Equivalence** (`PC01`, `PC02`): Vue and Svelte keep every product.
- **HTML stub** (`PC03`): a frontend row only.
- **Astro stub** (`PC04`): frontend and projection rows, no backend. An IDE
  demand is served without one.
- **Closure** (`PC05`): no frontend module reaches an `LC` module. The proof is
  structural, never a name scanner.
- **Missing backend** (`PC06`): a compile demand reports `UNSUPPORTED` and
  leaves every other product intact.
- **Facts without a compiler** (`PC07`) and **one registration authority**
  (`PC08`).

`WC01`–`WC06` specify the counts CPF1 reports against the baseline commit:

- frontend parses per parse key;
- registry dispatches per demand;
- projection and template-fact producer executions;
- compiler-backend executions, which are zero for every tooling-only demand;
- the `SourceTextCopy`, `CodeTransformRender` and `SourceMapBuild`
  attribution counts.

Projection executions, template-fact executions and the attribution counts
reuse existing readers. Registry dispatches are counted by CPF1 at the
registry entry. The frontend parse-entry count is counted by CPF1 at
`InstalledCarrierFrontend::parse` and published `pub` under `test-support`,
the same exposure as `take_projection_producer_invocations`, so the proof
entry `crates/verter_compiler/tests/cases/carrier_frontend_backend_split_proof.rs`
can read it. `registered_frontend_parse_count` is `#[cfg(test)] pub(super)`
and is not that source. `CarrierParse` attribution records parse bytes, not
the parse-entry count. `WC01` excludes `StandaloneCompiler::compile` and
`prepare`: those calls never enter `InstalledCarrierFrontend::parse`, and
they are `F-C13`. The growth ratio is the registered frontend entry on
`PC01` and `PC02` only. CPF1's cutover makes the direct route consume the
admitted frontend parse (`lower_vue_from_parsed` and
`lower_svelte_from_parsed` already do) instead of calling `parse_sfc` or
`parse_svelte`. The counter is not widened, and no second counter is added.
Wall-clock numbers, if any, come from a bench-m3 evidence run and never gate.

## Findings recorded for the receiving owners

- **Two identities for one carrier.** `project_registered_accepted` selects the
  frontend row, then rebuilds a `KnownRegisteredCompiler` from it, so the row
  and the struct both answer "which carrier" (`F-D02`, `D02`).
- **The host binding re-scans.** `registered_host_integration_for` has no
  production caller; `native_host_binding` re-scans the host catalog itself
  (`D02`, `D03`).
- **Parse once holds for the registries.** Backend and projection rows already
  consume the one admitted `FrameworkParseArtifact` through `M05`/`M15`.
  `StandaloneCompiler::compile` and `prepare` do not: they call `parse_sfc`
  and `parse_svelte` (`F-C13`). Their successor consumes that admitted parse,
  so separation does not require a second artifact and the charter's abort
  condition does not trigger.
- **UAK1 classes all of `vue_bridge` as `LC`.** Its frontend half (`M03`–`M06`,
  `P01`, `P06`, `P07`) is `L3`; `F-D01` splits the file.

## Acceptance evidence

Evidence selection: the change adds contract text and data only, so existing
coverage and bounded inspection discriminate it. The diff adds no test.
For the compile-request ownership repair, the architect ruling
`cpf0-r1-test-pin` (2026-10-09) accepts reviewed consistency of the inventory
and this decision as R1 evidence. Executable ownership validation remains
UAI0's later obligation under `UAI0-AC-R1`; it is not passing evidence here.

- **AC1 — ownership contract.** The inventory binds every method, product,
  outcome, consumer and displaced route to one existing, production-capable
  plan node, a successor path from CPF0 and a receiving acceptance ID.
  Contract-only nodes appear as `contractOwner`, never as `owner`; the
  receiving scope extension for compile-request consumers is ratified in
  amendment `RA01`. UAK0, UAK1 and VID0 rows are
  referenced, not re-owned. The executable validator is UAI0's
  (`UAI0-AC-R1`).
- **AC2 — positive contract.** Existing coverage pins the identity, provenance
  and ordering of the boundaries this decision names:
  - in `crates/verter_compiler/tests/cases/`: `vue_carrier_frontend`,
    `svelte_carrier_frontend`, `projection_catalog`,
    `compiler_capability_catalog`, `vue_runtime_backend`,
    `svelte_runtime_backend`, `vue_semantic_authority`,
    `svelte_semantic_authority` and `parse_diagnostic_determinism`;
  - the compile-fail fixtures `frontend_only_has_no_runtime_accessor`,
    `projection_only_has_no_runtime_accessor`, `grant_mint_is_private` and
    `host_epoch_forbidden_on_frontend`;
  - `compiler_layer_dependency_closure` and `no_session_dependency` for the
    crate edges;
  - `crates/verter_language/tests/cases/` (`parse_identity`,
    `registered_authorities`);
  - `framework_registry_complete` and
    `framework_surface_wire_executor_validates_first` in `verter_session`.

  New tests and the stub proofs belong to CPF1 (`CPF1-AC-R1`).
- **AC3 — incremental equivalence: not applicable.** No cache, cancellation,
  stale-publication or partial-result authority is touched, and no production
  byte changes. `PC06` binds "a partial result is never promoted warm" to CPF1.
- **AC4 — bounded work: not applicable.** No hot path changes. `WC01`–`WC06`
  bind the parse-once and no-hidden-work evidence to CPF1.

