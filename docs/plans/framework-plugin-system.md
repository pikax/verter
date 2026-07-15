# Implementation Plan — Framework Plugin System (adapters + extensions)

Status: REVIEWED (v3, post 1-Claude + 2-codex review + a codex confirmation re-review;
verdict: sound to proceed). Owner: TBD.
Supersedes the exploration scratch under `docs/exploration/framework-adapter-loading/`.

Make Verter the "Vite for frontend frameworks" of the LSP world: framework
**adapters** and framework **extensions** become **runtime-loaded, config-declared,
npm-distributable native plugins**, with third-party plugins **first-class
(capability parity with built-ins)** — preserving the single type-resolution
engine, the CodeTransform source-map authority, and the shared-codebase invariants.

> **Standing constraint:** this plan edits `verter_session` extensively. Per the
> owner's standing rule, **confirm before editing `verter_session`** at each phase.

---

## 1. Context (current state, de-staled per review)

Framework support is **compile-time today**: the session `FrameworkAdapterRegistry`
is built once at `host_construction.rs:186`; the compiler `CarrierCompilerRegistry`
is a process-wide `OnceLock` (`crates/verter_session/src/parse.rs:134`); language/
carrier-token rows are static in `crates/verter_language/src/registry.rs:152`. These
are **three separate authorities** that all must become runtime-generation-scoped —
replacing only the session registry strands a plugin's carrier before parse/codegen.

Already in place (do NOT re-introduce/delete): `FrameworkAdapterId(Arc<str>)` exists
(`verter_language/src/ids.rs`); the **request** wire already carries an open
`ComponentSelector.framework_adapter_id` (`typeinfo.proto:807`); `FrameworkAdapterDescriptor`
is stored **by value** in registry rows (`framework/registry.rs:72`) — only specific
**fields** are `&'static` (`descriptor.rs:67,116,118`); Svelte already registers a real
carrier/synth/api-projector/script-fact provider; the client-framework-manifest
baseline + the per-op typeinfo **validators** (`request_validation.rs`) + the audited
`resolve_named_symbol_with_audit`/`evaluate_type_expression_with_audit` methods already
exist (only the *framework-surface* graph executor is wired through the envelope).

What's missing: runtime discovery/load of external adapters+extensions; users
add/replace; a plugin transform/recognition seam; type-aware plugin power. There is
no pre-codegen transform hook (the only build-time source transforms are post-compile
JS string ops in `@verter/unplugin` `ssr-transforms.ts`, with no LSP counterpart).

User priorities (binding): performance-first (native machine code, zero-copy);
users add **or replace** plugins; third-party plugins a **powerhouse comparable to
native**; breaking changes allowed; best architecture.

---

## 2. Goals / non-goals / flagship decision

**Goals:** runtime/config/npm loading of adapters + extensions; de-privileged
built-ins (Vue/Svelte travel the runtime path; override by id); native `abi_stable`
cdylib loader (in-proc machine code, zero-copy facts, ABI-gated); extension plugins at
capability parity with built-ins; all invariants (§4) preserved.

**Non-goals (deferred — §11):** WASM / JS-in-Node / out-of-process tiers; a general
transformable template IR + structural template transforms; `ComponentOptionBag`;
broad program-analysis query families beyond current host producers; semantic-query
ops without a live producer (`Relation`, flow/contextual).

**Flagship = native `abi_stable` cdylib.** The unprimed comparative score ranked WASM
#1 *for an untrusted-npm marketplace* and native last on safety; under the owner's
trusted+perf weighting native is correct. The loader-agnostic contract keeps WASM/JS/
out-of-proc as **deferred, non-breaking** tiers — *iff* the v1 ABI is transport-neutral
(§5.6). Honest cost: in-proc native is unsandboxed (a bad plugin can crash the LSP) —
mitigated by trust gating, hash-pinned manifests, preflight load, slow-call watchdog.

---

## 3. Architecture overview

Two axes on one substrate. **The one-engine firewall:** resolution stays host-side in
the module-private `ExecutorResolveCtx` / `ProjectSemanticDispatch`. Adapters PLAN +
NORMALIZE over the closed `PlannedDemand` vocabulary; extensions RECOGNIZE + TRANSFORM
+ INJECT + DECLARE and may only ASK the engine via the read-only query API (immutable
DTOs, no resolver handle). No plugin resolves a type.

```
   shared substrate: runtime generation bundle · verter.config discovery ·
   abi_stable transport-neutral plugin ABI · identity/validation-split cache keys
        │                                                  │
   AXIS A — ADAPTER plugins (add/replace a framework)  AXIS B — EXTENSION plugins (augment)
   AdapterProvider -> the existing legs;               recognize + edit-ops + IDE-decl overlay +
   closed PlannedDemand (SvelteSurface ->              synth contributions + template augments +
   closed TypedIrSurface)                              read-only SEMANTIC QUERY API + tooling
```

---

## 4. Hard invariants (every phase; each names its guard)

1. **One engine.** No plugin holds `ProjectSemanticDispatch`/`StoreView`/resolver
   handle/OXC arena/`SemanticNodeId`. Plugins read resolved data only via the query
   API as immutable DTOs. Guard `plugin_surface_exposes_no_resolver_handle`.
2. **CodeTransform single source of truth.** Every generated byte is a host-replayed
   `CodeTransform`/`CodeGenOutput` op; **edit-op replay is the v1 third-party contract**
   (rendered code+map pairs are trusted-built-in/internal ONLY, never the third-party
   ABI). Guard `plugin_edits_route_through_codetransform` + `compile_audit_sourcemap`.
3. **Shared codebase / one plan.** A host-owned `ExtensionPlanSet` + per-target
   `ExtensionSourceView` is computed ONCE per file/env generation BEFORE the parse/
   codegen split; build (`BUNDLER`) and LSP (`IDE`) consume the same plan. Guard
   `extension_plan_computed_once_before_codegen_split`.
4. **Typed-IR only.** Structural recognition (OXC syntax/spans/exact import specifiers/
   resolved `ResolvedPackage`/lowered IR). Guard `plugin_recognition_is_typed_ir_only`.
5. **Cache: identity ≠ validation (R6/R21).** Plan-artifact IDENTITY keys are
   content-free deterministic fingerprints (registry hash, plugin id/version/config/
   schema, `path_context_hash`, `file_language_id`, adapter id, input-fact fingerprint).
   Semantic-query READ-DEPS are recorded **value-side** and validated through the
   existing `ReadSetSignature`/`validate_with_self_roots` rail — never as identity keys.
   Partial/budget/cancelled never warm; zero-cost when no plugin active. Guard
   `extension_plan_key_is_content_free_readdeps_validate_value_side`.
6. **Cross-platform.** Win/macOS/Linux; native binaries via `@verter/native`
   `optionalDependencies`; paths via `Path`/`PathBuf`. Guard `tracked_paths_are_portable`.
7. **No dual paths.** One runtime generation bundle; the compile-time-only authorities
   are deleted/generation-bound, not shimmed.

CRITICAL co-landing: any §4 invariant promoted to a `(CRITICAL)` heading in CLAUDE.md
or `/framework-adapters` MUST register its guard row in `CRITICAL_RULE_GUARDS` in the
SAME change (the `every_critical_rule_in_docs_has_registered_guard` meta-guard).

---

## 5. Shared native-plugin substrate

**5.1 Owned descriptor (field widening, NOT a twin).** The descriptor is already
stored by value; widen its `&'static` fields in place: `supported_surfaces: Arc<[FrameworkSurfaceKind]>`,
suffix fields `Arc<str>`/`Cow<'static,str>` (`descriptor.rs:67,116,118`). Built-in
`vue_descriptor()`/`svelte_descriptor()` change only at their literal sites
(`Arc::from(".ts")`). A native plugin's descriptor DTO is lowered at the loader
boundary immediately into this one canonical type. The byte-pinned TS generators keep
iterating built-ins (re-pin if literal forms change).

**5.2 One runtime generation bundle (the cutover).** Define
`PluginRuntimeGeneration { language_registry, minted_carrier_tokens,
carrier_compiler_registry, framework_adapter_registry, extension_registry,
classifier, watcher_config, source_loader_state, registry_hash }`, produced from
normalized config and installed atomically into the host. This replaces ALL three
authorities: `host_construction.rs:186` (session registry), the `OnceLock`
`CarrierCompilerRegistry::built_in()` (`parse.rs:134` — deleted or generation-bound),
and the static `verter_language` carrier rows (`registry.rs:152` — generation-scoped
so plugin carriers classify). LSP classifiers/watchers read from the generation.
**Reload = a generation barrier**, not just `ArcSwap`: cancel/re-key in-flight
scheduler + extension plans, rescan open files, resync type-provider buffers,
re-register watchers, invalidate affected caches. (`AdapterProvider`/`FrameworkExtension`
traits produce the rows; `built_in()` becomes a special case of `from_providers()`.)

**5.3 Discovery / config / trust.** A concrete `PluginLoadSpec { id, kind:
adapter|extension, package?, entry/binary, version, integrity_hash, config,
trust: workspace|trusted }` with override precedence (config > built-in by id).
Discovery: `verter.config.*` (workspace-root resolved via the VFS node-resolution —
no Node runtime needed) + LSP `initializationOptions` + unplugin options → one
normalized `Vec<PluginLoadSpec>`, mapped through `HostConfig`. Define config-file
watching + reload. **Native code loads only after trust gating + integrity-hash
validation + a preflight load** in a helper before the real host loads it.

**5.4 Wire migration (corrected, coordinated with U8).** The *request* selection is
already open (`ComponentSelector.framework_adapter_id`). The *response* migration is
**additive-first, breaking-later, coordinated with the U8-owed `graph` retag** on the
SAME message (`FrameworkSurfacePayload`, `typeinfo.proto:854`):
- Step A — the open-id addition IS additive: add `string framework_adapter_id = 6` to
  the payload; keep+deprecate `FrameworkTag framework = 3` (no break, off-tree clients
  keep decoding). Coordinate this with the U8-owed `graph` (field 4) carrier change in
  ONE migration — but note the `graph` change is NOT additive: do it as either
  (i) add a NEW field `TypeInfoGraphPayload graph_payload = 7` and deprecate
  `SemanticTypeGraph graph = 4` (keeps decode back-compat), OR (ii) declare a
  schema-versioned BREAKING change for the framework-surface graph payload and reject
  old payloads. Either way bump BOTH `FrameworkSurfacePayload.schema_version` AND the
  `SemanticTypeGraph.schema_version` constant + the supported-version set. Regenerate
  proto + TS; update `@verter/typeinfo` (`framework-surface.ts:150` `framework: FrameworkTag`),
  the MCP projector, NAPI/WASM, audit payloads, the byte-pinned
  `virtual-file-naming.ts` (keyed by `FrameworkTag` names → adapter-id keys), and
  supported-version + proto/TS-freshness tests.
- Step B (later, breaking): `reserved 3; reserved "framework";` once off-tree clients
  have migrated. Governed by the Typeinfo Wire Contract (reserved tag + name).

**5.5 Cache identity (replace the placeholder).** Replace `plugin_versions_hash`
(`virtual_file_pipeline.rs:111`, single producer feeding `WorldSnapshotDims.plugin_versions`
at `world_snapshot.rs:70`, already guard-pinned into snapshot identity) with a real
global plugin-registry hash. The per-file plan IDENTITY key adds content-free
fingerprints (§4.5); per-query read-deps validate value-side. **Generation-scope the
off-store framework caches** (`FrameworkSurfaceStore`/`FrameworkScriptCaches`, today on
registry rows — the U10 debt): move under `ProjectTypeStore`/singleflight or bind to the
generation hash, else dynamic registry reload serves stale.

**5.6 Transport-neutral ABI.** `verter_plugin_abi` defines v1 as **request/response
DTOs** + ABI-safe string tables / borrowed slices / **opaque handles with explicit
ownership + free functions**. FORBID Rust `Arc`/`String`/`Vec`/trait-objects/`Any`
across the boundary. The host-query "callback" is specified as **host-query RPC
semantics** (request id, explicit budget + cancellation receipts), NOT a sync Rust
fn-pointer assumption — so WASM/out-of-proc are genuinely non-breaking later. `libloading`;
content-addressed copy before load (Windows DLL-lock safe); ABI-version handshake
(mismatch → refuse, keep built-in); process-lifetime (no unload while sessions exist).

**5.7 Client activation model (stated).** A post-start `verter/frameworkManifest`
notification CANNOT add VS Code `contributes.languages`/activation events. v1 model:
**third-party plugins are limited to language ids/extensions the extension already
declares** (a broad carrier-extension allowlist + file-association fallback), with the
manifest notification driving dynamic document-selector/watcher/TS-plugin reconfig
within that envelope. New top-level language contributions remain a packaged-extension
change.

---

## 6. Axis A — Adapter plugins
`AdapterProvider` produces the existing legs (`CarrierCompiler`, `FrameworkSurfaceAdapter`,
`ScriptFactProvider`, `ComponentDefaultSynth`, `ComponentApiProjector`); built-in
Vue/Svelte and a third-party adapter both yield a `FrameworkRegistration`. Native
plugins use marshalling shims; the private `Arc<dyn CarrierParse>` carrier becomes an
`OpaqueCarrierBlob` + a loaded `CarrierAccessToken` minted in `verter_language`.
**CodeTransform across the boundary = closed edit-op replay (v1 third-party contract);**
rendered `(code, source_map)` pairs are trusted-built-in/internal only.
**`SvelteSurface` → a closed `TypedIrSurface`** with a DEFINED grammar: allowed
handles, a declared source-family id, validation rules, and exact lowering to existing
`SemanticQueryKey`/value domains — **no raw TS source, no raw `SemanticNodeId`, no
plugin-owned resolver semantics** (stays inside the no-`Raw`-arm + one-engine rule;
across the ABI it is validated typed-IR only).

---

## 7. Axis B — Extension plugins (parity)
- **Rich inputs** — an immutable `PluginInputSnapshot` (file/project/path/workspace
  context; navigable script + template fact graphs with **stable** ids + spans;
  imports; macros; lowered type IR; read-only component-meta facts).
- **Read-only semantic query API** — a closed `PluginSemanticQuery` returning immutable
  DTOs + a receipt. **v1 = live producers only**, each with an op-contract-table row
  (executor, backing `SemanticQueryKey`/component-surface source, env dims, read-dep
  recording, completeness, audit payload, NAPI/WASM/MCP/TS encoder, budget/cancel):
  `ResolveType`(`ResolveDecl`), `ProjectTypePath`(`ProjectPath`), `EvaluateExpression`,
  `ResolveMacroPayload`, `ComponentSurface`(component-meta materialization). Delivered
  by adding per-op host executors mirroring `resolve_framework_surface_with_audit`
  (the per-op *validators* + `resolve_named_symbol_with_audit`/
  `evaluate_type_expression_with_audit` already exist; the work is the wire executor +
  neutral `SemanticTypeGraph` encoder + envelope dispatch), backed by
  `ProjectSemanticDispatch::execute_read` under the existing `RequestContext`/budget/
  read-dep machinery. **Deferred/rejected in v1:** `ImportResolution` (route via the
  existing host import-resolution path as a separate query, not the graph engine),
  `Relation`, flow/contextual, any non-producing variant.
- **Op vocabulary** — `PluginPlan { edit_ops (target-tagged CodeTransform escape hatch,
  host-validated: in-bounds/recognized/non-overlapping/not-reserved), ide_decls,
  synth, template_augments, diagnostics, completions, code_actions }`.
- **IDE declaration injection = host-owned OVERLAY inputs, not just a TSX prelude.**
  Injected declarations enter the canonical overlay path (shallow processing +
  `parse_stable_hash` + `ModuleAugmentationIndex` + env invalidation + provider sync)
  so they affect BOTH tsserver/tsgo AND Rust semantic queries/component-meta. (The
  Svelte rune-prelude is prior art for prelude-offset *position mapping* only; the rail
  itself is framework-neutral.) Guard: an injected augmentation changes both a TS and a
  Rust query result in tests.
- **Synth contributions** — closed `SyntheticContribution::{DefineOptions, MacroFact,
  ComponentDefault}` at the `ComponentDefaultSynth` seam (`host_construction.rs:798`,
  which has the path) with `precedence`/`if_absent` (source wins via a small
  `source_component_name` fact). **One component-identity fact** is plumbed through ALL
  name paths: runtime (`compile/mod.rs:501`), the session TSC/public-API path
  (`virtual_file_pipeline.rs:1946`), IDE compile, and template-data — not just one site.
- **Template augmentation** — a side table keyed by **stable template node ids** (NOT
  raw arena `NodeId`, which is unstable across generations), consumed by runtime codegen
  + IDE JSX + template-data extraction + static analysis + **component-meta + fallthrough/
  root-inheritance** (a prop/attr augment affects `inheritAttrs`/multi-root/spreads, not
  only emitted bytes). Structural insert/wrap/remove DEFERRED to a real template IR (§11).
- **Activation gates** — `ImportSpecifier` (requires widening `ScriptFactSyntaxGate::ImportSpecifier(&'static str)`
  AND `ActiveProviderIndex.by_import_specifier` to an owned/interned id — a real change
  to a closed enum + the `script_fact_providers_zero_cost_on_miss` guard substrate),
  `CarrierLanguage`, `PathGlob`, `AlwaysOnForAdapter`, `All`/`Any`/`Not`. **`PathGlob`
  spec:** a named glob engine, normalized workspace-relative path basis, explicit case
  handling, evaluated at the session host layer (where `canonical_id` is available);
  Windows/case/rename tests required.
- **Composition** — fixed order semantic-inject → script-edit → template-transform →
  output-inject → type-enhance → tooling; within a class adapter-priority → plugin
  order → id → version → op-index; fail-closed conflicts (source > convention;
  overlapping destructive edits from different owners → diagnostic + drop lower;
  template op on a removed/missing node → skip + diagnostic).

---

## 8. Phased roadmap (file-level; each lists Changes / Deletions / Gate)

> Every phase's gate = the canonical Rust gate (`cargo nextest run --workspace` +
> `cargo test -p verter_session --tests`) + `cargo clippy --workspace -- -D warnings`
> + `cargo fmt --all --check` + (if TS touched) `pnpm test` + `pnpm install --frozen-lockfile`,
> PLUS the phase-specific discriminating tests named below. Confirm before editing
> `verter_session`.

**Phase 0 — Wire migration + descriptor field-widening + config/discovery (M–L).**
Files: `typeinfo.proto` (+ generated Rust/TS), `framework-surface.ts`, MCP projector,
NAPI/WASM, audit payloads, `virtual-file-naming.ts`, supported-version + proto/TS-freshness
tests (§5.4); `descriptor.rs` field widening (§5.1); `HostConfig` + `verter.config`
schema + `PluginLoadSpec` + discovery/watch (§5.3); `verter_lsp` init-options + the
`verter/frameworkManifest` notification + activation model (§5.7).
Deletions: `FrameworkTag`-as-runtime-namespace usages (response-side); none of the
existing open id (`FrameworkAdapterId`) — it stays.
Gate: wire-contract guards (oneof parity, proto/TS byte-freshness, reserved discipline),
decode round-trip for both old+new fields, supported-version gate both ways.

**Phase 1 — One runtime generation bundle + native loader + Vue/Svelte as providers (XL).**
Files: new `PluginRuntimeGeneration` (§5.2) in `verter_session`; `host_construction.rs:186`;
`parse.rs:134` (`OnceLock` deleted/generation-bound); `verter_language/registry.rs:152`
(generation-scoped rows); **ban/migrate every production `LanguageRegistry::global()`
/ static-classification call site (LSP classifiers/watchers, workspace resolver
helpers, session frontier / fallthrough / file-artifact paths) to read classification
from the installed `PluginRuntimeGeneration`** — leaving any static classification path
undercuts the generation bundle; new `verter_plugin_abi` (§5.6);
`libloading` loader + trust/hash/preflight (§5.3); `NativeCarrierCompiler`/surface/
script-fact/synth/api-projector shims; `OpaqueCarrierBlob` + loaded `CarrierAccessToken`
in `verter_language`; `SvelteSurface`→closed `TypedIrSurface` (§6) in `plan.rs:105`;
adapter `adapter_cache_key` into `FileArtifactStore`; reload generation barrier (§5.2).
Re-express built-in Vue/Svelte as `AdapterProvider`s.
Deletions: `CarrierCompilerRegistry::built_in()` process-wide authority; hardcoded
built-in carrier registration; `PlannedDemand::SvelteSurface` arm.
Gate (discriminating): config-loaded adapter routes parse/codegen; built-in **override
by id** uses the plugin; **unknown id + ABI-mismatch refused**; Vue/Svelte output
byte-identical (`rehoused_carrier_dispatch_drives_compile_byte_identical` + golden
`include_str!` hashes); reload-during-active-LSP-session test; no-new-resolve-engine
cluster green.

**Phase 2 — Extension lean core + source-view + `definePage` (L).**
Files: `ExtensionRegistry` + `ExtensionPlanSet` + per-target `ExtensionSourceView`
computed before the codegen split and threaded through `parse`/`eval_source`/
`compile_ide`/`template_data`/`compile_bundle` (`virtual_file_pipeline.rs` ~1688 demand
normalization; the `CarrierCompiler` call sites); recognition via `ScriptFactProvider`
with the widened `ImportSpecifier` gate; target-tagged `CodeTransform` edit-op escape
hatch + host validation; deterministic composition + fail-closed conflicts; per-file
plan IDENTITY hashing (§4.5). Ship `definePage` (Runtime strip + IDE declaration).
Deletions: the `@verter/unplugin` `ssr-transforms.ts` post-compile string strips for
anything an extension should own (state which stay + why).
Gate: `definePage` E2E (bundle clean + IDE hover/no-error + sourcemap round-trip);
**local `definePage` look-alike rejected** (resolved-package identity); overlap-conflict
tests; closed-edit-vocab + `plugin_surface_exposes_no_resolver_handle` guards;
both-targets-from-one-plan test.

**Phase 3 — Semantic query API (live producers only) (L).**
Files: per-op executors + neutral `SemanticTypeGraph` encoders + envelope dispatch
(reusing the existing validators + `resolve_named_symbol`/`evaluate_type_expression`),
backed by `execute_read`; read-dep recording into the plan VALIDATION record (not the
identity key); phase guards (queries only in plan + LSP-request phases), budgets
(default Navigate/Shallow; Expanded gated), dedup, completeness flags; the op-contract
table (§7); NAPI/WASM/MCP encoders for the new ops.
Gate: query returns neutral DTO (no `SemanticNodeId` leak) — guard
`plugin_query_returns_neutral_dto_no_node_id`; **edit to a queried dependency invalidates**
the plan; budget/cycle safety; deferred ops (`Relation`/flow/`ImportResolution`) return
typed Unavailable, not a stub.

**Phase 4 — Synth + component identity + template augmentation (M–L).**
Files: closed `SyntheticContribution` at `synth.rs`/`host_construction.rs:798`; the
single component-identity fact plumbed through `compile/mod.rs:501` +
`virtual_file_pipeline.rs:1946` + IDE + template-data; `TemplateAugmentations` side
table keyed by stable template node ids, consumed by runtime + IDE + template-data +
static-analysis + component-meta + fallthrough; `PathGlob` gate at the session host
layer. Ship path-derived `defineOptions({name})` + a template prop example.
Gate: `index.vue → parent-folder name` E2E (runtime `name` + TSC public API + IDE);
**explicit `defineOptions` wins**; template-augment reflected in both codegens AND
component-meta/fallthrough (`inheritAttrs`/multi-root/spread tests); **`path_hash`
invalidation on rename with unchanged content**.

**Phase 5 — Native ABI parity for extensions (L).**
Files: extensions ride the `verter_plugin_abi` transport-neutral surface (§5.6);
zero-copy `PluginInputView` fact slices + string table; host-query RPC (budget/cancel
receipts); `PluginPlanOut`; identical surface for in-process + cdylib.
Gate: **in-process vs cdylib parity conformance** (same logic, identical plan);
zero-copy fact bench; ABI version negotiation; crash/error reporting + adapter-disable.

**Phase 6 — Hardening + conformance + DX (M–L).**
Files: audit events (query cost/expansion depth/cache deps/op conflicts);
`@verter/plugin-conformance` (sourcemap/eval-source/determinism/closed-vocab/parity);
`npm create verter-plugin`; register all new guards in `CRITICAL_RULE_GUARDS` (co-landed).
Gate: full canonical gate + `pnpm test`; new guards green + self-tested;
`builtin_and_plugin_capability_parity` conformance.

**Phase 7 — Deferred (non-breaking).** WASM/JS/out-of-proc tiers; real template IR +
structural template ops; `ComponentOptionBag`; broader relation/program-analysis
queries; component-meta component-name wire field (if a consumer needs it).

---

## 9. Legacy deletions (by file/function)

- `host_construction.rs:186` `built_in()` as sole construction → special case of
  `from_providers()` via `PluginRuntimeGeneration`.
- `parse.rs:134` `CarrierCompilerRegistry::built_in()` process-wide `OnceLock` → deleted
  / generation-bound.
- `verter_language/registry.rs:152` static carrier rows → generation-scoped.
- static `LanguageRegistry::global()` classification call sites (LSP / workspace
  resolver / session frontier-fallthrough-file-artifact) → generation-scoped reads
  (guard `no_static_language_classification_outside_generation`).
- `FrameworkTag`-as-runtime-namespace on the RESPONSE wire + consumers
  (`framework-surface.ts`, MCP projector, generated bindings, `virtual-file-naming.ts`
  `FrameworkTag` keys); old field reserved in Step B (§5.4).
- `PlannedDemand::SvelteSurface` (`plan.rs:105`) → closed `TypedIrSurface`.
- `plugin_versions_hash` placeholder (`virtual_file_pipeline.rs:111`) → real registry hash.
- `@verter/unplugin` `ssr-transforms.ts` string strips for plugin-ownable transforms
  (document the few that legitimately stay, with no LSP counterpart needed).
- Off-store `FrameworkSurfaceStore`/`FrameworkScriptCaches` registry-row hosting →
  generation-scoped / on the project store (U10).
- No re-export shims for any relocated/renamed symbol.

---

## 10. Verification (discriminating; red before green)

- Canonical Rust gate + clippy + fmt + `pnpm test` + frozen lockfile each phase.
- **New guards** (registered in `CRITICAL_RULE_GUARDS`): `plugin_surface_exposes_no_resolver_handle`,
  `plugin_edits_route_through_codetransform`, `plugin_recognition_is_typed_ir_only`,
  `plugin_query_returns_neutral_dto_no_node_id`, `extension_plan_key_is_content_free_readdeps_validate_value_side`,
  `extension_plan_computed_once_before_codegen_split`, `builtin_and_plugin_capability_parity`,
  `template_augment_is_side_table_no_structural`, `no_static_language_classification_outside_generation`,
  plus wire-contract guards for the retag.
- **Red/green fixtures** (must fail pre-change, pass post-): config-loaded plugin routing;
  built-in **override by id**; **unknown id + ABI-mismatch refusal**; **in-process vs
  cdylib parity**; **local `definePage` look-alike rejection**; **unchanged-content
  path-rename invalidation**; plugin hash/config **cache-miss**; generated proto/TS
  **freshness**; reload during an active LSP session; an injected IDE declaration
  affecting **both** a TS and a Rust query result; a template augment affecting
  **fallthrough/component-meta** (not just emitted bytes).

---

## 11. Honest gaps / risks / deferred

- **Biggest real gap:** structural template transforms (insert/wrap/remove) need a
  template IR (Phase 7); augmentations on existing nodes ship now. Not hidden.
- **Native trust:** in-proc native is unsandboxed — trust gating + hash-pin + preflight
  + slow-call watchdog + disable-on-fault; WASM (Phase 7) is the sandboxed path.
- **Query cost / read-dep explosion:** closed enum, default Navigate/Shallow, gated
  Expanded, per-plugin budgets + fan-out caps, host dedup, value-side read-dep storage,
  phase guards.
- **ABI evolution:** abi_stable layout checks + version negotiation; cache keys include
  ABI + plugin version; clean major bumps, no shims; transport-neutral so WASM/out-of-proc
  are non-breaking.
- **Dynamic registry reach:** the single generation bundle + reload barrier (§5.2) is
  the load-bearing mechanism; if any authority is missed, plugin carriers strand.

---

## 12. POC evidence (worktree)

- `poc-native-adapter-boundary/`: cdylib loaded via `libloading`; in-proc dispatch ~7.6 ns;
  zero-copy 256 KiB ~0.36 µs vs copy-in ~3.84 µs (10.8× boundary tax); user plugin
  **replaces a built-in by id** (ABI-gated); non-overridden ids stay built-in.
- `poc-extension-model/`: recognize-once → target-divergent emit (Runtime strips
  `definePage`; IDE keeps + injects a typed declaration) with prelude-offset position
  mapping preserved.

---

## 13. Resolved decisions / open items

- **CodeTransform boundary:** RESOLVED — closed edit-op replay is the v1 third-party
  contract; rendered pairs are trusted-internal only (per review codex-A-6).
- **Wire migration:** RESOLVED — additive Step A coordinated with the U8 `graph` retag;
  breaking reserve in Step B (§5.4).
- **Open:** Phase 7 ordering (WASM vs template IR); whether to also land this plan as a
  `docs/arch/` design + a `/framework-adapters` skill update.
