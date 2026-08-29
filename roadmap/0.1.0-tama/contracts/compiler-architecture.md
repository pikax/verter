# Compiler architecture source contract

This digest-bound requirement-atom transfer owns compiler-wide decisions that precede individual compiler block charters. Dispatch packets include only the subset needed by the selected node; this contract remains the common authority for cross-block invariants.

The operational skill
[`.claude/skills/compiler-codegen/references/authority-policy-demand.md`](../../../.claude/skills/compiler-codegen/references/authority-policy-demand.md)
points here. It is not a second constitution.

This lock is constitution-only: **zero production LOC**. Live production still
routes through the combined carrier-compiler registry and host compile routes
until later migration nodes delete those seams. Combined-registry identity is
displaced as *authority*, not preserved behind aliases.

---

## Sole owner

Final owner: **verter_compiler capability traits plus immutable registration
catalog**.

Displaced as authority (must be deleted or structurally rejected by later
migration nodes; not dual-run here):

- combined `CarrierCompiler` trait and `CarrierCompilerRegistry`
- mixed framework/options buckets on one runtime option struct
- tooling-only runtime stubs that pretend a missing compiler product exists
- `CompileTarget` bitflags as compiler product/pipeline selector

No sixth compiler authority exists. CSS-family syntax and lossless tokens stay
with the CSS syntax crate. TypeInfo / typed IR stay shared analysis machinery,
not a framework semantic authority.

The combined `CarrierCompiler` trait is a **temporary selector** only: it
selects the live adapter row for the current migration seam. It is not an
authority. No new methods may be added to it.
`CompileTarget` bitflags (`STYLE` / `SCRIPT` / `TEMPLATE` / `TSX` / `TSC` / `TEMPLATE_DATA` and presets `BUNDLER` / `IDE` / `ANALYSIS` / `META`) are a temporary compiler pipeline selector, not an authority and not a product identity. Product identity is the `ProductKind` 1:1 cells. No new `CompileTarget` flags. The session-owned `CompileTarget` cache-key discriminant is not a compiler authority and is out of this lock; it must not be treated as a seventh product bus.

---

## Five authorities

| Authority | Role |
| --- | --- |
| `CarrierFrontend` | Parse, registered geometry, unregistered parse artifacts, parse diagnostics (artifact-retained), syntax reject (no ParseAdmission) |
| `FrameworkSemanticAuthority<FrameworkEpoch>` | Per-framework interpretation: eval-source, template facts, **framework style meaning** |
| `ProjectionBackend` | IDE companion, public-API, and declarations (TSC / `.d.ts`) projection |
| `RuntimeCompilerBackend<FrameworkEpoch>` | Runtime emit with statically selected targets; emits **admitted facts only** |
| `FrameworkHostIntegrationBackend<FrameworkEpoch, HostEpoch>` | Host/unplugin/session integration; composes parse + semantic into `CompileAdmission`; publication |

Parse diagnostics (identity, provenance, completeness, deterministic order) are CarrierFrontend artifacts retained on the parse artifact and admitted by ParseAdmission. syntax reject is fail-closed parse (no ParseAdmission). Host integration may publish those artifacts on the diagnostic channel. They are not a ProductKind cell, not ProductKind::Analysis, and not a compile-product leftover.

`CompileArtifactSet` is a later schema; this constitution names it as the
eventual artifact relation, not a sixth authority.

---

## Catalog

- **Key:** `adapter × epoch × capability`.
- **Epoch type is the authority.** `FrameworkSemanticAuthority<E>`,
  `RuntimeCompilerBackend<E>`, and `FrameworkHostIntegrationBackend<E, HostE>`
  take a typed epoch. `CarrierFrontend` and `ProjectionBackend` do not; their
  `register_frontend` / `register_projection` constructors still take
  `E: FrameworkEpoch`. Catalog `FrameworkEpochId` / `HostEpochId` are derived
  from that type (`E::ID` / `HostE::ID`). A backend cannot be registered under
  a different epoch spelling: there is no independent epoch-value argument.
- **Table:** process-lifetime immutable. Built once. No insert, replace, or
  unload after process start.
- **No runtime plugin.** Dynamic ABI / plugin load is forbidden.
- **Identity methods** (`adapter_id`, `carrier_language_id`, epoch, capability
  flags) live on the **catalog row**, not on the combined trait.

A mixed-options request does not become a catalog row. Owner-local request
types stay on the owning authority.

---

## Current method → final owner

Every `CarrierCompiler` method and every production registry/host/unplugin
dispatch maps to exactly one final owner. A combined pass is **not** a
seventh owner and **not** a third product bus: each requested `ProductKind`
leg has the owner named for that kind.

| Current caller / method | Final owner |
| --- | --- |
| `CarrierCompiler::adapter_id` | catalog row identity |
| `CarrierCompiler::carrier_language_id` | catalog row identity |
| `CarrierCompiler::parse` and closed adapter parse switches | `CarrierFrontend` |
| `CarrierCompiler::eval_source` / blanking | `FrameworkSemanticAuthority` |
| `CarrierCompiler::template_data` / `TemplateFacts` | `FrameworkSemanticAuthority` (`ProductKind::Analysis` fact leg) |
| `CarrierCompiler::compile_ide` | `ProjectionBackend` (`IdeCompanion` / `PublicApi` legs) |
| `CarrierCompiler::compile_bundle` | **combined product pass**, not an owner. Host integration issues one `CompileAdmission`; each requested kind is owned as below. |
| `compile_bundle` runtime legs (`RuntimeClient`, `RuntimeServer`) | `RuntimeCompilerBackend` |
| `compile_bundle` IDE / public-API legs (`IdeCompanion`, `PublicApi`) | `ProjectionBackend` |
| `compile_bundle` declaration leg (`Declarations`) | `ProjectionBackend` (TSC splice / `.d.ts` shape) |
| `compile_bundle` analysis / facts leg (`Analysis`) | `FrameworkSemanticAuthority` (admitted facts; no codegen bus) |
| `standalone::StandaloneCompiler` | `FrameworkHostIntegrationBackend` composing the same per-kind owners as `compile_bundle` (direct/core host, not a second catalog) |
| `assembly::vue_module` / framework module topology (how fragments become one runtime module) | `RuntimeCompilerBackend` |
| `assembly::publish` / virtual-file / host decoration of already-emitted fragments | `FrameworkHostIntegrationBackend` |
| `tsc::*` (declaration emit, module specifiers, TSC script splice) | `ProjectionBackend` (`Declarations`) |
| Style syntax, tokens, dialect IR | CSS syntax crate (J) |
| Framework-owned style meaning (`v-bind`, scoped, `:global`, matcher consume, Svelte CSS match/prune) | **`FrameworkSemanticAuthority` only** |
| Runtime style emit (admitted style facts → CSS/runtime helpers) | `RuntimeCompilerBackend`; **does not re-interpret** |
| `style_planner` / `compile::style_usage` / `script::css_vars` / `svelte::runtime::css` | **displaced combined interpreters** (today they mix meaning + emit). Successor split: meaning → `FrameworkSemanticAuthority`; emit → `RuntimeCompilerBackend`. Not a dual-owner row. |
| Neutral macro DTO produce (typed `define*` / `withDefaults` surfaces) | shared TypeInfo / typed IR; not a compiler authority |
| Framework meaning of macros (join admitted parse + DTO → eval-source / template facts / style-meaning facts) | `FrameworkSemanticAuthority` only |
| Neutral macro DTO emit-consume (`MacroRuntimeBundle` → runtime script; `MacroTscBundle` → declarations; IDE public names) | `RuntimeCompilerBackend` / `ProjectionBackend`; does not re-resolve or re-interpret. Never a compiler-owned type resolver. Not a dual-owner row. |
| Parse diagnostics (mapped `LanguageDiagnostic` on the parse artifact) | `CarrierFrontend` (artifact-retained). Host integration publishes the admitted artifact. Not a `ProductKind` cell and not `ProductKind::Analysis`. | |
| `CompileRequest` construction / capability matrix | catalog-selected capabilities; request remains the typed demand document |
| Mixed framework/options buckets | **not a catalog row**; split into owner-local request types |
| `CarrierCompilerRegistry::built_in` / `from_compilers` | displaced selector; successor is the immutable catalog |
| `CarrierCompilerRegistry::get` / `contains` / `registered_adapter_ids` / `compiler_for_carrier_language` | catalog lookup (host integration consumes) |
| `CarrierCompilerRegistry::project_registered` | `CarrierFrontend` (registered geometry) |
| `verter_session` parse path (`parse.rs` registry `OnceLock`) | `FrameworkHostIntegrationBackend` composing `CarrierFrontend` |
| `verter_session` `host_compile` / `ensure_compiled` / `virtual_file_pipeline` `compile_bundle` | `FrameworkHostIntegrationBackend` composing `CompileAdmission`; product legs follow the `compile_bundle` rows above |
| `packages/unplugin` compiler entry | `FrameworkHostIntegrationBackend` |

Duplicate analysis is forbidden: parse once per admission, interpret once per
framework epoch, emit once per selected target.

---

## Policy and compatibility

`CompilePolicy::{Default, Optimized}`.

### `DefaultCompilationContractId` spelling and versioning

Spelling (ASCII, lowercase, dot-separated):

```
default.<framework-epoch>.<product-family>.v<unsigned-integer>
```

Examples (illustrative, not a live registry):

- `default.vue-sfc-v3.runtime-client.v1`
- `default.vue-sfc-v3.runtime-server.v1`
- `default.vue-sfc-v3.ide-companion.v1`
- `default.vue-sfc-v3.public-api.v1`
- `default.vue-sfc-v3.declarations.v1`
- `default.vue-sfc-v3.analysis.v1`
- `default.svelte-5.runtime-client.v1`
- `default.svelte-5.declarations.v1`

Rules:

- `<framework-epoch>` names one semantic epoch (Vue SFC generation, Svelte
  major, host epoch is **not** in this id).
- `<product-family>` is **1:1 with live `ProductKind`**. Closed spelling:

| `ProductKind` | `<product-family>` cell |
| --- | --- |
| `RuntimeClient` | `runtime-client` |
| `RuntimeServer` | `runtime-server` |
| `IdeCompanion` | `ide-companion` |
| `PublicApi` | `public-api` |
| `Declarations` | `declarations` |
| `Analysis` | `analysis` |

  There is **no** `facts` dump cell and **no** many-to-one grouping of kinds
  into one versioned family. Client vs server, IDE companion vs public API vs
  declarations, and analysis each version independently. A later grouping
  would be a constitution amendment and must name shared versioning
  consequences; this lock does not group.
- `v<n>` is the Default **behavior** version for that cell. Increment `n` when
  Default observable behavior for that cell changes (including a permitted
  cheap local-fact correction on that kind). Do not reuse a prior `n` for a
  different meaning.
- The id versions **Default** only. Optimized never shares this id.

### Equivalence matrix (product × grade)

Equivalence is **per `ProductKind` cell**, never a global byte-identity claim.

| `ProductKind` | Default grade | Meaning |
| --- | --- | --- |
| `RuntimeClient` | structural + behavioral | Client emit vs official/oracle: helper topology, memo/effect, DOM/hydration, attribute routing, diagnostic order. Cosmetic JS carrier formatting is not a finding. |
| `RuntimeServer` | structural + behavioral | Server/SSR emit vs official/oracle under the same structural/behavioral grade, versioned **separately** from client. |
| `IdeCompanion` | mapped TypeScript/JSX surface | One generated TS/JS/JSX surface for script + supported template expressions; provider features map back through `ProviderPositionMapper`. Unmapped regions fail closed. |
| `PublicApi` | public-API shape | Exported instance/public contract (`$props` / emit / slot / expose surfaces as consumed by importers). Not the IDE companion file and not runtime helper topology. |
| `Declarations` | `.d.ts` / TSC splice shape | Declaration-file and TSC macro-bundle splice text. Not public-API projection and not runtime emit. |
| `Analysis` | exact admitted fact identity | Template facts, style-meaning facts, eval-source: identity, provenance, completeness, deterministic order. Not parse diagnostics and not a dump bucket for other products' leftovers. |

### Cheap local Default facts

`Default` may use stronger cheap **component-local** facts and may correct
prelocked upstream gaps **without project I/O** (example: a local alias-proven
reactivity case). That correction:

- is demand kind **`SemanticFact`**;
- is admitted only by token **`SemanticAdmission`**;
- is issued **only** by `FrameworkSemanticAuthority` over an **already
  admitted `ParseAdmission`** (already-parsed component; no new file graph);
- bumps the `DefaultCompilationContractId` cell of the `ProductKind` whose
  Default observable behavior changed.

Forbidden even for Default corrections:

- a backend-private type environment, compiler-local type resolver, or
  companion type store on `RuntimeCompilerBackend` or `ProjectionBackend`;
- a second resolve around TypeInfo / typed IR (no compiler-owned walk that
  re-resolves what shared analysis already refused or did not admit);
- host/session TypeInfo execution from inside the compiler crate.

### Intentional divergences

Existing backend deviation records remain **authoritative**. This constitution
**adds none**. A later node that changes Default observable behavior records
the deviation on the owning backend, not here.

### `Optimized`

`Optimized` is a truthful **future** capability only:

- no implementation
- no admission token
- no `DefaultCompilationContractId`
- **fail-closed as unsupported** when requested
- never a stub runtime
- never a silent fallback into Default under an Optimized name

---

## Demand and admission

Demand universe is finite and **monotonic**: a later stage may add reasons,
never retract a satisfied demand or reopen an unrelated file graph.

### Closed demand kinds

Any other kind requires a constitution amendment before implementation.

| Kind | Asks for |
| --- | --- |
| `ParseRegion` | syntax artifacts for a named parse region / parse key |
| `SemanticFact` | framework interpretation of an admitted parse (eval-source, template facts, style meaning, framework meaning of macros). Interpretation may read the host-resolved dependency-neutral macro DTO as input. It is not emit-consume of that DTO. |
| `CompileProduct` | a selected live `ProductKind` (the six cells above); not a combined bus |

### Reason-edge types

Edges that justify a demand are typed. Closed set:

| Edge | Meaning |
| --- | --- |
| `ParseRegionReason` | this parse region is required to satisfy a named demand |
| `SemanticFactReason` | this fact is required from the framework authority over admitted parse |
| `CompileProductReason` | this compile product is required for the request |

### Resumption

Resumption key = **demand identity** (kind + stable demand id). Resumption
does not re-discover work. A later stage may attach additional reason edges to
an existing demand; it may not mint a new identity for the same work.

### Admission tokens

Charter-faithful choice: **one issuer** for `CompileAdmission`. Host
integration composes parse + semantic into that token. Product backends
**consume** it. They do not mint a same-named type of their own.

| Token | Issued by | Admits | Consumed by |
| --- | --- | --- | --- |
| `ParseAdmission` | `CarrierFrontend` | syntax artifacts for one parse key | semantic authority; host composition |
| `SemanticAdmission` | `FrameworkSemanticAuthority` | framework interpretation over admitted parse | host composition |
| `CompileAdmission` | **`FrameworkHostIntegrationBackend` only** (composes the two prior tokens) | emit and publication of requested products | `ProjectionBackend`, `RuntimeCompilerBackend` |

Host integration does not re-parse or re-interpret to mint a fourth token.
There are not three product-scoped `CompileAdmission` types.

---

## Semantic authority

Each framework semantic epoch has exactly one authority:

- **Vue** epoch: Vue `FrameworkSemanticAuthority`
- **Svelte** epoch: Svelte `FrameworkSemanticAuthority`

There is no global framework semantic authority.

### TypeInfo versus framework interpretation

| Surface | Owner |
| --- | --- |
| TypeInfo / typed IR / shared `verter_semantic` graph | shared analysis machinery |
| Framework meaning of macros, template facts, style, eval-source | the epoch's `FrameworkSemanticAuthority` |

TypeInfo is not a framework authority. Framework authorities **build on**
shared TypeInfo; they do not replace it and they do not fork a second resolver.

### Generic compiler surfaces

Generic surfaces (capability traits, catalog schema, compile request, assembly
of already-emitted fragments) depend only on:

- capability traits
- shared IR (`verter_semantic` / TypeInfo as data, not as Vue/Svelte meaning)
- the dependency-neutral macro DTO

They must not import framework-private semantic types.

Framework backends may consume shared typed IR and the macro DTO.

The runtime compiler must not own a second analyzer: no compiler-local type
resolver, companion type environment, or host type-store replica — including
as a vehicle for cheap Default fact corrections.

### Residual (honest)

The crate-graph guard
(`compiler_production_closure_does_not_reach_host_session_or_transport_crates`)
proves **host / crate closure only**: `verter_compiler` production edges do
not reach session/query/LSP/NAPI/FFI/WASM and still reach `verter_semantic` +
`verter_macro_dto`. It does **not** prove that the runtime compiler owns no
in-crate second analyzer, and it does **not** prove the generic-versus-
framework module split.

The **in-crate** generic versus framework split is locked in **prose** until
typed catalog / capability types exist. This constitution does **not** claim
the in-crate firewall is proven.

---

## Framework style meaning

Interpretation of framework style constructs (`v-bind`, scoped, `:global`,
matcher consume, and Svelte equivalents) is **solely**
`FrameworkSemanticAuthority`.

Runtime emit consumes **admitted style facts only**. It must not re-interpret
matchers, scoped, `v-bind`, or `:global` from source or from CSS syntax IR.

CSS-family syntax, tokens, and dialect IR stay with the CSS syntax crate.

---

## Identity and representation

- Snapshot-local IDs are dense and local to one admitted snapshot.
- Authored offsets and optional lineage are separate from dense IDs.
- Lossless trivia / tooling recovery is excluded from compiler hot-path
  contracts.
- Physical materialization of an artifact is optional; identity does not
  require a lossless sidecar.
- There is no universal compiler IR, no mandatory reactivity IR, no compiler
  ABI, no native preprocessor, and no external OXC artifact as a compile
  product.

---

## Incremental and bounded-work (AC3 / AC4)

This lock does not own or change incremental, cache, cancellation,
stale-publication, or partial-result authority. **AC3 N/A.**

This lock does not own or change a hot path; it adds no parse/resolve/plan/emit
work. **AC4 N/A.**

---

## Forbidden

Vue/Svelte V2 implementation, CSS matcher changes, native preprocessors,
project-wide optimization, dynamic plugin/ABI, preserving combined authority
behind aliases, dual-running authority, successors implemented in this lock,
re-interpreting framework style meaning in the runtime emitter, claiming the
in-crate generic/framework firewall is proven, adding demand kinds without
amendment, minting product-scoped `CompileAdmission` types, treating
`compile_bundle` or `facts` as a product owner, grouping `ProductKind` cells
without an amendment, backend-private type environments for Default
corrections, treating SemanticFact as emit-consume of the macro DTO, treating parse diagnostics as ProductKind::Analysis or as a seventh ProductKind, treating CompileTarget as a product owner.
