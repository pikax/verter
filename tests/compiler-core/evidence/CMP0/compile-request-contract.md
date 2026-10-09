# Compiler request, policy, compatibility and identity contract

This decision replaces mixed compile options and one broad output identity
with a typed per-framework request, an exact default contract, an
option-impact classification and six stage identities. Today, framework
compiler emitters and per-node target dispatch own these choices. The final
and sole owner is the data-oriented common compiler substrate with
framework-native planning.

It describes the repository at `docs(arch): record the parser decision,
ownership, reuse and lineage (#812)`, 2026-10-09. It follows the docs-only
rule in [the kernel README](../../../../docs/arch/kernel/README.md): it
changes no production route and adds no check. It builds on the
[identities](../../../../docs/arch/kernel/identities.md),
[configuration](../../../../docs/arch/kernel/configuration.md),
[demand](../../../../docs/arch/kernel/demand-activation.md),
[parser](../../../../docs/arch/kernel/parser-ownership.md) and
[carrier frontend/backend](../../../../docs/arch/kernel/carrier-frontend-backend.md)
decisions and does not re-own anything they assign.

## Machine-readable products

The reviewed contract data lives in `tests/compiler-core/CMP0/products/`:

| File | Holds |
| ---- | ----- |
| `compile-request-inventory.v1.json` | Rules `RQ01`–`RQ10`, `DC01`–`DC06`, `SK01`–`SK10`, `OB01`–`OB05`, `NB01`–`NB06`, `MG01`–`MG09`; outcomes `CMP-O01`–`CMP-O18`; consumers `CMP-C01`–`CMP-C14`; displaced routes `CMP-D01`–`CMP-D17`; referenced routes, category coverage, empty populations, findings `CMP-F01`–`CMP-F16`, the proposed receiving-charter amendment `RA02`, plan consumers and transferred obligations `CMP-T01`–`CMP-T04` |
| `option-impact-classification.v1.json` | The seven option-impact classes, rules `OI01`–`OI06`, and one row per caller-settable request field (`OC-T..`, `OC-P..`, `OC-V..`, `OC-S..`) plus the host-resolved execution inputs (`OC-H..`), each with its class and its routing status at the described head |
| `compile-request-case-table.v1.json` | Negative cases `CN01`–`CN13`, positive cases `CP01`–`CP05` and work counters `WC01`–`WC04` |

Every `successorPath` starts at CMP0 and follows predecessor edges in the
controller-owned plan. VID0, CFG0, CPF0, DEM0 and PAR0 rows keep their owners;
CMP0 references them by id.

## Method

1. Read every field of `CompileRequest`, `FrameworkCompileRequest`,
   `VueCompileRequest`, `SvelteCompileRequest`, the `CompileProduct` requests
   and the host execution inputs, and followed each to the stage that reads
   it on the standalone route and on the host integration route.
2. Found every production constructor and consumer of the request and every
   older option shape still feeding compilation, across `verter_compiler`,
   `verter_session`, the transports, `verter-tsc`, the LSP, MCP and unplugin.
3. Followed every compile cache key and output identity to its inputs and
   consumers.
4. Located the production sites of the charter's three displaced categories
   and of the retained mixed conversion named by CMP1's charter.
5. Gave each outcome, consumer and displaced route one owner whose charter
   allows production work and names that route, on a successor path from
   CMP0.

## What exists at the described head

- **A typed request already exists.** `compile_request::CompileRequest`
  holds a product set, one `FrameworkCompileRequest` (Vue or Svelte), a
  semantic profile, a framework-neutral identity block and host assembly
  axes. Construction refuses empty or duplicate products, `SSR × Vapor`,
  `inline × SSR` and Vue-only IDE axes on Svelte. `VueOption` and
  `SvelteOption` classify every official option row by *support*
  (`supported canonical`, `derived`, `unsupported fail-closed`, …), not by
  *invalidation impact*.
- **No policy, contract id or version field.** There is no `CompilerPolicy`,
  no `Default`/`Optimized`, no contract id. Each framework is pinned to one
  compatibility domain (`core@3.6.0-rc.3`, `svelte@5.56.8`) as informational
  capability dispositions.
- **Admission is route-local.** The host integrations refuse options their
  bundle cannot route (`refuse_unproducible_vue_options`,
  `refuse_unproducible_svelte_options`, `unroutable_host_request_axis`). The
  standalone route does not: it admits twelve Vue fields and reads none of
  them (`CMP-F01`), and reads `hoistStatic` and the Svelte `css` and
  custom-element descriptor that the host route refuses (`CMP-F02`).
- **The request is flattened back into mixed shapes.** The host routes turn
  the request into the cross-framework `RuntimeCompileOptions`, which is
  turned into `CodegenOptions`, `ResolvedVueCompileOptions` and the
  compiler's `CompileTarget` bits (`derive_legacy_vue_options`), or into
  `SvelteRuntimeOptions` through a typed-enum → string → enum round trip.
  `compile_from_parsed_legacy` bypasses the request (`CMP-D12`).
- **The session keeps a cross-framework option bag and one broad key.**
  `CompileProfile` holds Vue fields, Svelte fields (none set by any
  production producer, `CMP-F07`) and `CompileTarget` presets. Its whole
  value is hashed by `compile_profile_hash` with `std DefaultHasher` into a
  u64 that keys the compile slots and is folded into the supplied-block
  token (`CMP-D13`, `CMP-D14`, `CMP-F03`).
- **Parse identity is already typed and stable.** `verter_identity`
  `ParseKey` with `SyntaxProfileId` is canonical-encoded and pinned; Vue
  delimiters and custom elements enter it, Svelte keeps only `loose`.
- **Artifacts carry lineage, not qualification.** `ArtifactId` is minted from
  (unit, product, language, name); target and policy appear nowhere on a
  published artifact (`CMP-F05`). unplugin keys its caches by filename only.
- **Emitters dispatch per node.** The Vue template walker calls
  `&mut dyn TemplateCodeGen` for every node; the Svelte compile builds a
  broad runtime IR for the whole component before narrowing; Vue paths deep
  copy `ParsedSfc` and lower the whole SFC twice for client plus server.

## Request and refusal vocabulary (CMP0-A)

The canonical envelope:

```text
CompileRequest
    exact source/content basis      (SourceUnitId, SourceRevision, ContentId) + supplied-input digests
    requested products and targets  a set; each product names its target
    CompilerPolicy                  Default(DefaultCompilationContractId) | Optimized (reserved)
    DefaultCompilationContractId    exact release, target, contract revision
    common execution controls       cancellation, budgets, audit capture; never identity
    typed framework request         VueCompileRequest | SvelteCompileRequest | (SolidCompileRequest, TS emit request)
```

- **One envelope, owner-local framework requests** (`RQ01`, `RQ02`). No field
  of one framework exists on another framework's request or on a shared
  product request. The Vue-only IDE axes that sit on `IdeProductRequest`
  today are refused on Svelte at construction and move to the Vue request
  when CMP1 lands the envelope.
- **Three admission outcomes** (`RQ05`–`RQ07`): `Admitted`, `NeedInputs` and
  `Unsupported`. `NeedInputs` names each required external input by type and
  resumes on the same basis; it never publishes or warms. `Unsupported`
  carries a typed reason rendered through each part's own `Display`.
- **One admission** (`RQ08`). Every production route reaches the same
  admission, which applies construction, routing and capability refusals. A
  route-local refusal list is a second admission authority.
- **No ignored field** (`RQ09`). Every accepted field is read by its class's
  stage on every route; a field no stage reads is refused on presence.
- **One field per fact** (`RQ10`). `vue.ssr` beside a `RuntimeServer` product,
  `svelte.dev` beside `isProduction`, and `componentId` on Svelte requests
  are duplicated or unread authorities (`CMP-F06`).

## Default contract registry (CMP0-B)

- `CompilerPolicy` is one closed type, `Default(DefaultCompilationContractId)`
  or `Optimized`. The source plan's `CompilePolicy` is the same type (`DC01`).
- Public `Default` normalizes at admission to an exact
  `DefaultCompilationContractId` = (VID0 `ReleaseId`, target kind, contract
  revision), recorded in artifact provenance (`DC02`).
- The registry has one row per (exact release, target). Each row lists
  per-stage contract epochs and its intentional divergence records. A
  revision that changes one stage advances only that stage's epoch (`DC03`).
- No ambient default: an unresolved release is `unsupported-version`;
  floating tags and ranges never decode; no family-wide version switch
  (`DC04`, VID0 `R06`, `R07`).
- Divergences are typed and pre-registered by the framework lock:
  presentation-only, more precise maps, or diagnostic wording (`DC05`).
- `Optimized` is reserved. Until OPT0 ratifies a scope, admission returns
  `Unsupported(Optimized)` with zero work, no key and no cache write (`DC06`).

The framework locks fill the rows: VCP0 (Vue), SCP0 (Svelte), SXC0 (Solid 2).

## Option-impact classification (CMP0-C)

| Class | Key it enters first | Examples |
| ----- | ------------------- | -------- |
| `Parse` | `ParseKey` (through `SyntaxProfileId`) | Vue `delimiters`, `isCustomElement`; Svelte `loose` (refused) |
| `Semantic` | `SemanticKey` | `filename`, `componentId`, Vue `propsDestructure`, `cssModules`; Svelte `runes`, `namespace`; host-resolved facts |
| `CompileStructure` | `CompileStructureKey` | products, `inline`, `styleProcessing`, analysis demand, Vue `ignoreEmpty` |
| `TargetPlan` | `TargetPlanKey` | `isProduction`, Vue `backend`, `whitespace`, `hoistStatic`, `cacheHandlers`; Svelte `css`, `fragments`, `preserveWhitespace`; IDE `strictSlots` |
| `Emit` | `EmitKey` | `forceJs`, map demand, Vue `runtimeModuleName`, `genDefaultAs`, `styleTrim`; Svelte `discloseVersion` |
| `Terminal` | `TerminalKey` | `ssrModuleId`, `hmrStrategy`, output, presentation and serialization profiles |
| `Refused` | none | Vue `parsePad`, `babelParserPlugins`, `compatConfig*`; Svelte `accessors`, `immutable`, `hmr` |

The tree stays lossless (PAR0 `PD05`), so whitespace condensing and comment
policy are `TargetPlan`, not `Parse`. Every row of the companion file has one
class (`OI01`). A row enters its class's key and, by chaining, every later key
of the same product (`OI02`); a framework row enters only that framework's
keys (`OI03`); map demand is `Emit` and map encoding `Terminal` (`OI04`);
execution controls enter no key (`OI05`). An option whose impact cannot be
classified is `Refused` until an owning amendment classifies it.

CFG0 `CF-H04` experimental flags stay host inputs. The host maps them onto the
Vue IDE axes (`OC-P08`, `OC-P09`); no profile-scoped key is introduced
(`MG06`).

## Stage identities (CMP0-D)

```text
ParseKey             existing verter_identity ParseKey (re-exported, never re-minted)
SemanticKey          = H(sorted ParseKeys of demanded regions, semantic profile, Semantic options, semantic epoch, semantic admission basis)
CompileStructureKey  = H(SemanticKey, product demand-closure digest, CompileStructure options, structure epoch)
TargetPlanKey        = H(CompileStructureKey, target, TargetPlan options, target epoch)
EmitKey              = H(TargetPlanKey, Emit options, MapMode, emit epoch)
TerminalKey          = H(EmitKey, Terminal options, terminal epoch)
```

- Six distinct nominal types (`SK01`). `ParseKey` is PAR0's `G07` identity;
  the compiler re-exports it (`SK02`, `CMP-F09`).
- Each product owns its own chain from its own demand closure. Products
  share a prefix exactly when their inputs to it are equal (`SK04`).
- Canonical bytes come from the `verter_identity` `CanonicalEncoder` with one
  domain tag per stage, fixed field numbers, absent distinct from present,
  set-like values sorted and deduplicated. Never `Debug`/`Display`,
  `serde_json`, a `std` hasher, a timestamp, a backend identity or "latest"
  (`SK05`).
- Map encoding never invalidates semantics (`SK06`); a framework option never
  re-keys another framework (`SK07`).
- Complete-only: degraded outcomes never publish under a key (`SK08`), and
  incremental equals fresh across edits and reverts (`SK09`).
- CMP0K defines the types and composition law; CMP1 derives them; CMP4E keys
  `EmitPlan` by `EmitKey` (`SK10`).

## Reserved optimized basis (CMP0-E)

```text
OptimizationRequestBasis    known before execution: request basis up to TargetPlanKey + optimization policy revision
OptimizationObservationSet  discovered during analysis: ordered observed workspace facts + digest
ArtifactBasis               request basis + observation digest + decision digest
```

Lookup is by `OptimizationRequestBasis`; each stored candidate's observation
set is then validated against the captured workspace basis, so lookup never
needs the future read set (`OB04`). CMP0K declares the three types only,
unconstructible outside the compiler crate; there is no traversal, proof
engine or cache until OPT0 (`OB05`).

## Named boundaries

| Rule | Boundary | Contract |
| ---- | -------- | -------- |
| `NB01` | `CompileRequest` | `RQ01`–`RQ10`; construction is the single admission |
| `NB02` | `CompilerPolicy` | `DC01`–`DC06` |
| `NB03` | `DemandSet` | the finite per-request specialization of DEM0 `P4`: per product, the closed set of required parse, semantic, style, map, planning and emission capabilities with reason edges, closed before execution; its closure digest enters `CompileStructureKey` |
| `NB04` | `RegionId` | VID0 `I06`, the only region identity that crosses a stage, a demand entry, a segment anchor or a qualifier. Compile-structure regions are snapshot-local dense ids of a distinct type, never spelled `RegionId`, never in a key. Lifetime classes: `Request`, `Snapshot`, `Retained`, `Published` |
| `NB05` | `EmissionSegment` | no framework field and no identity; anchors are (`SourceUnitId`, `RegionId`, exact UTF-8 span); `EmitPlan` keyed by `EmitKey`; no map work under `NoMap` |
| `NB06` | `ArtifactQualifier` | (`ProductKind`, target, `DefaultCompilationContractId`, `TerminalKey`) on every published artifact beside `ArtifactId` lineage; `ArtifactId` alone never addresses an artifact in a cache or host map |

## Migration and deletion ledger (CMP0-F)

| Rule | Old shape | New shape | Route |
| ---- | --------- | --------- | ----- |
| `MG01` | `derive_legacy_vue_options`, `CodegenOptions`, `ResolvedVueCompileOptions`, compiler `CompileTarget`, `compile_from_parsed_legacy` | Vue plan inputs from `VueCompileRequest` and `TargetPlanKey` | `CMP-D12` |
| `MG02` | `RuntimeCompileOptions`, `IdeCompileOptions` | each backend receives its own typed request plus host assembly axes | `CMP-D12` |
| `MG03` | session `CompileProfile` framework fields | typed request; host-only input remains (UAK0 `S13`) or the type goes | `CMP-D13` |
| `MG04` | session `CompileTarget`, `request_from_target`, `compileWithAudit(target)`, `compileMany` render profile | typed requests | `CMP-D13` |
| `MG05` | `compile_profile_hash` and the profile-hash keys | per-product `TerminalKey` | `CMP-D14` |
| `MG07` | route-local refusal lists | the single admission | `CMP-D15` |

No adapter survives its last consumer, and no dual admission remains
(`MG08`). Wire changes keep refusals typed and name the written field; they
ride CPF0's `RA01` transport migration (`MG09`).

## Outcomes and owners

| Id | Outcome | Owner |
| -- | ------- | ----- |
| `CMP-O01` | Typed envelope, policy, contract id, three admission outcomes, one admission | CMP1 (`CMP1-AC2`) |
| `CMP-O02` | Classifier enforced at admission, with its negative tests | CMP1 (`CMP1-AC2`) |
| `CMP-O03` | Stage identity types and canonical encoding | CMP0K (`CMP0K-AC1`) |
| `CMP-O04` | Reserved optimized-basis types | CMP0K (`CMP0K-AC-R1`) |
| `CMP-O05` | Per-product key derivation | CMP1 (`CMP1-AC2`) |
| `CMP-O06` | Complete-only publication, incremental equals fresh | CMP1 (`CMP1-AC3`) |
| `CMP-O07` | `Optimized` refuses with zero work | CMP1 (`CMP1-AC1`) |
| `CMP-O08` | Whether `Optimized` ever executes | OPT0 (`OPT0-AC1`) |
| `CMP-O09`–`CMP-O11` | Vue, Svelte, Solid 2 Default contract rows | VCP0, SCP0, SXC0 (`AC1`) |
| `CMP-O12` | TS option catalog mapped onto option-impact classes | TSC0 (`TSC0-AC1`) |
| `CMP-O13` | Closed compiler `DemandSet` | CMP1 (`CMP1-AC2`) |
| `CMP-O14` | Compile-structure region ids and lifetime classes | CMP2 (`CMP2-AC2`) |
| `CMP-O15` | `EmitPlan` keyed by `EmitKey`, physical `NoMap` | CMP4E (`CMP4E-AC2`) |
| `CMP-O16` | `ArtifactQualifier` on every published artifact | CMP4 (`CMP4-AC2`) |
| `CMP-O17` | Static target executors in the common machinery | CMP3 (`CMP3-AC1`) |
| `CMP-O18` | Per-stage work counters | CPER1 (`CPER1-AC2`) |

Consumers `CMP-C01`–`CMP-C14` are in the inventory. The envelope and its
transport and `verter-tsc` consumers keep CPF0's binding to CMP1 under
`RA01`. unplugin's request builder and caches go to BND1 (`BND1-AC1`); the TS
emit request to TSE1 (`TSE1-AC3`); the Solid request to SXC2 (`SXC2-AC1`).

## Displaced routes recorded here

| Route | Category | Unit | Owner |
| ----- | -------- | ---- | ----- |
| `CMP-D01` | dynamic dispatch inside node loops | Vue template walker `&mut dyn TemplateCodeGen` per node; per-identifier target flags | VCP6 (`VCP6-AC1`) |
| `CMP-D02` | whole-tree materialization | broad `SvelteRuntimeIr` for every compile | SCP6 (`SCP6-AC1`) |
| `CMP-D03` | whole-tree materialization | `ParsedSfc` deep copies, `CompileTarget::BUNDLER` full codegen, second whole lowering for client plus server | VCP6 (`VCP6-AC1`) |
| `CMP-D04` | whole-tree materialization | eager whole-template expression parse | VCP6 (`VCP6-AC1`) |
| `CMP-D05` | constructed-output reparse | assembly `final_module_parse_errors` | CMP4 (`CMP4-AC1`) |
| `CMP-D06` | constructed-output reparse | `validate_generated_client_module` | SCP6 (`SCP6-AC1`) |
| `CMP-D07` | unqualified artifact assembly | `VerterCompileResult` envelope and its conversions | VCP6 (`VCP6-AC1`) |
| `CMP-D08` | unqualified artifact assembly | `RuntimeCompileOutput` fixed script/template blocks | VCP6 (`VCP6-AC1`) |
| `CMP-D09` | unqualified artifact assembly | Svelte `ClientModule` attaching CSS | SCP6 (`SCP6-AC1`) |
| `CMP-D10` | unqualified artifact assembly | session `AssembledVueModule` and fixed virtual-node slots with hard-coded languages | VCP6 (`VCP6-AC1`) |
| `CMP-D11` | unqualified artifact assembly | publication addressed by `ArtifactId` lineage only | CMP4 (`CMP4-AC1`) |
| `CMP-D12` | mixed request/policy conversion | `RuntimeCompileOptions`, `derive_legacy_vue_options`, `compile_from_parsed_legacy` and their round trips | CMP1 (`CMP1-AC1`) |
| `CMP-D13` | mixed request/policy conversion | session `CompileProfile` option bag and its conversions | CMP1 (`CMP1-AC1`) |
| `CMP-D14` | unqualified cache identity | `compile_profile_hash` and the profile-hash keys | CMP1 (`CMP1-AC3`) |
| `CMP-D15` | ignored or route-local option | route-local refusal lists; standalone ignoring admitted fields | CMP1 (`CMP1-AC1`) |
| `CMP-D16` | ignored or route-local option | `vue.ssr`, `svelte.dev`, Svelte `componentId` | CMP1 (`CMP1-AC1`) |
| `CMP-D17` | unqualified cache identity | unplugin filename-keyed caches | BND1 (`BND1-AC1`) |

CPF0's `F-D06` (the runtime bundle carrying the IDE companion and template
facts) stays CPF1's. The only per-node trait-object site is the Vue template
walker; carrier-level `Arc<dyn CarrierParse>` calls run once per file, and the
IDE and Svelte walks dispatch statically. No `HashMap<String, _>` or
`serde_json::Value` option bag and no transform registry exists (empty
populations in the inventory).

## Findings recorded for the receiving owners

- **Twelve Vue fields are accepted and ignored** on the standalone route
  (`CMP-F01`), and three fields mean different things on the two routes
  (`CMP-F02`). CMP1 folds admission into one function (`CMP-D15`).
- **The session key is broad and unstable** (`CMP-F03`): a Svelte-only change
  re-keys every Vue slot, and the u64 is not stable across toolchains.
- **The compile transaction's input basis omits options and products**
  (`CMP-F04`); identity of the output rides only the output digest.
- **Artifacts alias across targets** (`CMP-F05`); CMP4's qualifier resolves it.
- **Duplicated or unread authorities** (`CMP-F06`), **dead Svelte profile
  fields** (`CMP-F07`), and an **asymmetric parse-class gate** on the
  canonical session route (`CMP-F08`).
- **`ParseKey` already exists** (`CMP-F09`); **`RegionId` is VID0's**
  (`CMP-F10`); **`CompilePolicy` and `CompilerPolicy` are one type**
  (`CMP-F11`).
- **CMP1's write scope does not cover its recorded deletion** (`CMP-F12`).
  The plan already records CMP1 as owner of the mixed conversion, but the
  F10 scope covers only CMP1's module homes and RA01's transport paths.
  Amendment `RA02` is proposed through question
  `cmp0-ra02-cmp1-cutover-scope`; the binding stands on the existing
  ownership until the ruling.
- **VCP6 and SCP6 surfaces** list only their new module homes while their
  deletion lists name the legacy routes recorded here (`CMP-F13`).
- **TSC0's projections are not stage classes** (`CMP-F14`).
- Two identities outside CMP0's graph are recorded, not bound: a `Debug`
  rendering in `CarrierInventory::artifact_identity_token` (`CMP-F15`, CPF1)
  and the cross-framework `Requested` syntax-profile arm (`CMP-F16`).

## Acceptance evidence

This change adds contract text and data only, so existing coverage and
bounded inspection are the evidence. The diff adds no test.

- **AC1 — ownership contract.** The inventory binds every outcome, consumer
  and displaced route to one existing plan node, a successor path from CMP0
  and a receiving acceptance id. Every displaced-route owner is a
  production-capable node whose deletion list names the route. CPF0, PAR0,
  DEM0, VID0 and CFG0 rows are referenced, not re-owned. The executable
  validator belongs to CMP5 (`CMP5-AC1`, `CMP-T03`).
- **AC2 — positive contract.** Existing coverage pins the identity,
  provenance and ordering of the boundaries named here:
  - `verter_language` `tests/cases/parse_identity.rs`:
    `parse_key_canonical_bytes_and_digest_are_pinned`,
    `syntax_profile_canonical_bytes_and_digest_are_pinned`,
    `vue_custom_element_order_and_duplicates_are_irrelevant`,
    `svelte_profile_has_no_vue_only_option_dimensions`;
  - `compile_request` unit tests: `vue_option_classification_counts_match_the_committed_tsv`,
    `svelte_option_classification_counts_match_the_committed_tsv`,
    `every_unsupported_fail_closed_row_is_unrepresentable_on_vue_compile_request`,
    `requesting_ide_companion_does_not_couple_to_runtime_source_map`;
  - `crates/verter_compiler/tests/cases/capability_matrix_compile_request_coverage.rs`
    and `framework_option_wire_paths.rs`;
  - `assembly/artifact_schema_tests.rs`
    `artifact_set_rejects_aliasing_and_incomplete_relations_or_provenance`
    and the `vue_module` / `svelte_module` revert-restores-identity tests;
  - `standalone_prepared_tests.rs`
    `compile_prepared_rejects_changed_vue_delimiters_as_stale` and
    `compile_batch_aba_cb_slot_correspondence_preserves_requested_product_and_digest`.

  New and extended tests belong to CMP1 (`CMP-T01`) and CMP0K (`CMP-T02`),
  as the case table assigns.
- **AC3 — incremental equivalence: not applicable.** No cache, cancellation,
  stale-publication or partial-result authority is touched, and no
  production byte changes. `SK08`, `SK09`, `CN09` and `CP05` bind
  complete-only publication and incremental-equals-fresh to CMP1
  (`CMP1-AC3`).
- **AC4 — bounded work: not applicable.** No hot path changes. `WC01`–`WC04`
  name the counters the receiving owners report through the CPER1 ledger.
