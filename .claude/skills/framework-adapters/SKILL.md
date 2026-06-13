---
name: framework-adapters
description: "Verter framework-adapter substrate — FrameworkAdapterRegistry, FrameworkAdapterDescriptor + virtual-file naming column, facts/carrier-only FrameworkAdapterCtx, the framework-surface executor (resolve_framework_surface_with_audit), the two-pass script-fact seam, ComponentDefaultSynth, and Vue as the reference adapter"
---

# Framework Adapter Substrate

Multi-framework component support is ONE shared adapter substrate, not a
per-framework semantic fork. The substrate lives in
`crates/verter_session/src/framework/` (+ the executor under
`crates/verter_session/src/typeinfo/framework_surface/` and the
syntax-capture half under
`crates/verter_semantic/src/analysis/framework_facts.rs`). Vue is the
REFERENCE adapter: re-housed as a true plan/normalize adapter, not a
privileged hardcoded path.

The canonical rule text is the **Framework Adapter Substrate (CRITICAL)**
section in `CLAUDE.md` (registered in the R6 meta-guard registry); this
skill is the module map + contract reference behind it.

## Module map

| Path | Owns |
|---|---|
| `framework/descriptor.rs` | `FrameworkAdapterDescriptor` (id, tag, supported surface kinds, carrier language, `VirtualFileNaming` column) + `vue_descriptor()`. Reuses the prost wire enums (`FrameworkTag`, `FrameworkSurfaceKind`) — no parallel host taxonomy. |
| `framework/registry.rs` | `FrameworkAdapterRegistry` (built once at host construction) + `FrameworkRegistration` (descriptor, optional carrier leg, synth, api-projector, script-fact providers, `SurfaceRegistration{Adapter,Deferred}`) + the `TagDisposition` table + `ActiveProviderIndex`. `framework_registry_complete` walks every wire tag. |
| `framework/ctx.rs` | `FrameworkAdapterCtx` — the facts/carrier-only adapter surface with EXACTLY two ops (`carrier_for`, `script_facts_for`). |
| `framework/surface_store.rs` | Generic `FrameworkSurfaceStore<K,B>` + `FullKey<K>` + erasure (`ErasedFrameworkSurfaceStore`) + the `FrameworkSurfaceDtoBundle` marker. Strict same-generation + fact-rail warm reads. |
| `framework/synth.rs` | `ComponentDefaultSynth` trait + parse-domain `ComponentDefaultSynthCtx` + `VueComponentDefaultSynth`. |
| `framework/script_facts.rs` | The resolved-validation half: `ActiveProviderIndex`-gated `resolve_script_facts`, the content-addressed candidate store, the fact-rail-validated resolved-fact store. |
| `framework/api_projector.rs` + `api_projectors/vue.rs` | `ComponentApiProjector` trait + the Vue leg delegating to `render_vue_public_api_legacy`. |
| `framework/virtual_file_naming_ts.rs` | Renders the byte-pinned TS mirror of the `VirtualFileNaming` column. |
| `verter_semantic/.../framework_facts.rs` | The syntax-capture half: `ScriptFactProvider` trait, closed `ScriptFactSyntaxGate`, candidate set, the zero-cost dispatcher param. |
| `typeinfo/framework_surface/{executor,plan,results,graph_export,vue_exec}.rs` | The executor entry, the closed plan/result vocabulary, the first `SemanticTypeGraph` encoder, and the relocated Vue resolution delegates. |
| `typeinfo/adapters/vue/adapter.rs` | `VueFrameworkAdapter` — plan/normalize only. |

## Descriptor + virtual-file naming column

`FrameworkAdapterDescriptor` is a registry row's immutable identity half.
`VirtualFileNaming { ide: Option<IdeSuffixPolicy>, api_suffix,
testing_api_suffix, sidecar_suffixes }` is the SINGLE authority for an
adapter's virtual-file suffixes (`IdeSuffixPolicy` is the closed
`Fixed` / `JsxConditional` pair). The structural rule
`testing_api_suffix.is_some() ⇒ api_suffix.is_some()` holds (a testing-API
file is a mode of the API file). The committed TS mirror
`packages/typescript-plugin/src/generated/virtual-file-naming.ts` is
RENDERED from this column (`render_virtual_file_naming_ts`) and BYTE-PINNED
by `virtual_file_naming_ts_freshness` (regen path under
`VERTER_UPDATE_VIRTUAL_FILE_NAMING_TS`). The LSP / ts-plugin naming
derivations are CONSUMERS of the column; consumer rewiring is a later
vertical, but the column + mirror + freshness pin land here so the two
cannot drift.

## The framework-surface executor

`VerterHost::resolve_framework_surface_with_audit(TypeInfoGraphRequest)` is
the SOLE audited entry for the `GRAPH_OPERATION_FRAMEWORK_SURFACES`
operation. It rides the EXISTING typeinfo graph envelope — no proto /
schema-version change. Flow:

1. `validate_type_info_graph_request` FIRST (op/payload-arm match, schema
   echo, nested framework-surface validator). A malformed envelope returns
   the typed `error` arm BEFORE any registry lookup or semantic dispatch
   (`framework_surface_wire_executor_validates_first`).
2. Intern `selector.framework_adapter_id` → registry lookup. Unknown id ⇒
   typed `MalformedPayload` (NO new error variant).
3. Resolve the selector (default export via the synthesized `default`;
   named export via the shallow inventory, strictly validated against the
   export table). A named-export framework surface is gated on the
   descriptor capability `supports_named_export_surfaces` (REGISTRY DATA,
   NOT an `is_vue()` branch in the neutral executor body —
   `framework_surface_executor_body_carries_no_privileged_framework_branch`);
   an adapter that does not support per-export surfaces rejects it as a typed
   `MalformedPayload`. Vue resolves the default-export component only, so it
   sets the capability `false`.
4. Requested set is ALWAYS `ALL_FRAMEWORK_SURFACE_KINDS` (the wire request
   carries no requested-kind field) — exactly one entry per known kind.
5. `SurfaceRegistration::Deferred` ⇒ every kind structurally UNSUPPORTED.
   `Adapter` ⇒ `plan_surfaces` over the facts/carrier-only ctx, the
   executor resolves each `PlannedDemand` through the module-private
   `ExecutorResolveCtx` (EXHAUSTIVE match, no wildcard) via the ONE shared
   type-resolution engine, then `normalize`, then `graph_export`.

The executor is NOT a second resolver: it plans, dispatches to the shared
engine, and encodes. `PlannedDemand` is a closed 4-variant taxonomy
(`PublicTypeInstance` / `MacroPayload` / `PathProjection` / `ShallowSurface`)
— no `Custom` / `Raw` arm, no source text, no OXC handles, no raw
`SemanticQueryKey`s. `ResolvedOutcome` (Resolved / Partial / Unsupported /
Missing) maps DIRECTLY onto the wire `SUPPORTED` / `PARTIAL` / `UNSUPPORTED`
status; a supported-empty kind stays distinct from an unsupported kind.

All SIX surface kinds resolve through the ONE shared object-surface
projection + a thin per-kind normalize: `defineProps`/`defineEmits`/
`defineSlots`/`defineModel` AND the object surfaces `defineOptions<T>()` /
`defineExpose<T>()`. The options/expose type argument projects to the SAME
one-level object surface (`resolve_vue_macro_surface_with_ctx`, no
special-case beyond `defineModel`) and normalizes via
`object_members_from_typeinfo_surface` into `OptionsSurface`/`ExposeSurface`
named members — SUPPORTED-with-members, NEVER unsupported-because-present.
Consistency invariant: a kind in an adapter's `supported_surfaces` whose
macro is USED resolves SUPPORTED, never a content-dependent support flip
(`every_supported_kind_resolves_supported_when_its_macro_is_used`).

`graph_export.rs` is the FIRST `SemanticTypeGraph` wire producer — a pure,
ZERO-DISPATCH, bounded shallow projection of resolved data. Named refs mint
`GraphSymbolNode` + `GraphReference{symbol_id}`; structural unencodables
degrade to `GraphOpaque` — never a fabricated ref, never a re-resolution.

## Facts/carrier-only adapter ctx

`FrameworkAdapterCtx` holds the adapter's REGISTRATION row + the host and
exposes EXACTLY two ops:

- `carrier_for::<T: CarrierParse>(canonical) -> Option<Arc<T>>` — the
  adapter's typed parse carrier. Returns `None` cleanly for a carrier-less
  adapter (`registration.carrier` is `None`) — never a forged token. Drives
  parse-domain artifact materialization internally (ensure-loaded → read
  the `framework_parse` slot → token-gated downcast) and hands back ONLY the
  typed carrier, never `FrameworkParseArtifact` / `IndexedReady`.
- `script_facts_for::<T: FrameworkScriptFactPayload>(canonical) -> Option<Arc<T>>`
  — drives the resolved-validation half on demand.

It never resolves types, indexes a file, runs OXC, calls
`ProjectSemanticDispatch`, or reads a `StoreView`. Pinned by
`framework_adapter_ctx_closed_surface`.

## Two-pass script-fact seam

The seam splits across the crate that owns each domain:

- **Syntax-capture half** (`verter_semantic::analysis::framework_facts`).
  `ScriptFactProvider::capture(ScriptCandidateCx)` collects candidates from
  the live OXC program — SYNTAX-ONLY: may touch the OXC AST +
  `lower_ts_type`, MUST NOT resolve imports or read capability bits
  (`script_fact_capture_is_syntax_only`). The neutral dispatcher
  (`build_script_analysis_with_scope_from_program_with_providers`) takes an
  `active_providers` slice; an EMPTY slice is the byte-identical pre-existing
  path with ZERO capture work (`script_fact_providers_zero_cost_on_miss`).
  `ScriptFactSyntaxGate` is closed + exact-valued (`CarrierLanguage` /
  `ImportSpecifier`) — no predicate arm.
- **Resolved-validation half** (`framework/script_facts.rs`). Driven on
  demand by `script_facts_for`. The `ActiveProviderIndex` (rebuilt per
  registry construction; `is_empty()` fast path) is the shared gate
  authority both the per-registration selection and the registry-wide index
  apply. Candidates content-address on `(canonical, content_hash,
  parse_env_hash, parser_version, file_language_id, provider_id,
  provider_version)`; resolved facts validate on a sub-key
  `(canonical, provider_id, provider_version, consumed_capability_bits,
  project_identity, resolve_env_hash)` — NO `lib_env_hash` / `type_env_hash`.
  `validate` receives NEUTRAL data (`ResolvedValidationCx`: candidates,
  resolved-import targets, a capability lookup) so the trait stays free of
  session resolver types. Publication is `SignatureAdmission::Cacheable`-only
  (overflow ⇒ `ReturnOnly`, no warm); the cold tracer observes the owner's
  `ImportRoute` fact (a re-route stale-serves otherwise). The provider
  rejects userland look-alikes and refuses emission when a consumed
  capability bit is OFF.

NO production provider registers in this program — Vue's macro analysis
stays inside the shallow pass, so the seam is exercised by an in-tree
fixture provider. A later framework vertical's provider drives the resolved
path in production.

## Component-default synth

`ComponentDefaultSynth::synthesise(ComponentDefaultSynthCtx)` synthesises a
component's default-export value symbol from PARSE-DOMAIN inputs only
(canonical id, resolved language row, macros, syntax-capture candidates) —
it never names the resolved-validation fact types
(`component_default_synth_parse_domain_only`; the ctx is whole-struct
destructure-pinned). The host's `inject_component_default_into_shallow_state`
selects the synth leg by the file's resolved `FileLanguage` adapter id at
the shallow-analysis injection points; `VueComponentDefaultSynth` wraps the
unchanged `synthesise_vue_default_value_symbol`. A typeinfo-evaluation
scratch (`verter://typeinfo/…`) has NO resolved framework language but must
synthesise the inlined scope's `default`; it routes to the registry's unique
synth-bearing adapter via `framework_registry().synthesizing_adapter_id()`
(REGISTRY DATA — never a hardcoded `FrameworkAdapterId::vue()` literal in the
neutral selector body; `neutral_default_injection_derives_scratch_adapter_from_registry`).

## Vue as the reference adapter

Vue resolution bodies relocated wholesale from
`typeinfo/adapters/vue/{public_type,surface}.rs` into
`framework_surface/vue_exec.rs`; those files + `store.rs` are DELETED with
NO re-export shim or alias under `adapters::vue` (`vue_relocation_no_shim`).
`VueShallowMetadataStore` / `VueMacroDtoKey` / `VueMacroDtos` /
`VueMacroDtosEntry` are RETIRED (the neutral `MacroSurfaceDtos` +
`FrameworkSurfaceStore<VueSurfaceKey, MacroSurfaceDtos>` replace them;
`retired_symbols_absent_from_production_source`). The retained
`impl VerterHost` Vue methods (`resolve_vue_public_type`,
`resolve_vue_macro_surface`, `vue_macro_dtos`) stay public API and CONVERGE
with the executor's private resolve ops on the same relocated `vue_exec`
delegates — ONE semantic path, two entries. `VueFrameworkAdapter` itself
holds no resolve entry point: `plan_surfaces` emits `PlannedDemand` data,
`normalize` consumes resolved results.

The public-API surface dispatches through the registry api-projector leg:
`get_public_api_with_mode` classifies the alias-resolved canonical by its
runtime-loaded `FileLanguage` and routes through
`framework_registry().api_projector_for(adapter_id)` — no `is_vue()` branch.

## Bindings (NAPI / WASM / MCP / TS)

- NAPI `resolveFrameworkSurfaceWithAudit(Buffer) -> { response, auditRecord }`
  (protobuf `TypeInfoGraphRequest` in, `TypeInfoGraphResponse` + audit out).
- WASM mirror `resolveFrameworkSurfaceWithAudit(&[u8]) -> { response, auditRecord }`.
- MCP `get_framework_surface` tool — projects the typed payload to JSON.
- TS `packages/typeinfo/src/framework-surface.ts` —
  `decodeFrameworkSurfaceResponse` decodes the wire payload into a per-kind
  `FrameworkSurface` (supported-empty distinct from unsupported, member
  names resolved through the graph string table). All bindings are thin
  adapters over the single host executor.

## Guards

| Guard | Pins |
|---|---|
| `framework_registry_complete` | every wire tag → registered adapter or explicit `TagDisposition` row |
| `framework_surface_wire_executor_validates_first` | validation precedes registry lookup / selector resolution |
| `framework_adapter_ctx_closed_surface` | exactly two pub ctx ops; no resolver tokens in `ctx.rs` |
| `component_default_synth_parse_domain_only` | synth ctx is parse-domain; no stage-2 fact types |
| `script_fact_capture_is_syntax_only` | the capture surface carries no resolved-import / capability data |
| `script_fact_providers_zero_cost_on_miss` | empty active-provider set is byte-identical zero-cost |
| `virtual_file_naming_ts_freshness` | the generated TS mirror is byte-equal to the rendered descriptor column |
| `vue_relocation_no_shim` | no re-export shim for relocated Vue resolution; deleted files stay deleted |
| `retired_symbols_absent_from_production_source` | `VueShallowMetadataStore` / `VueMacroDtoKey` / `VueMacroDtos` / `VueMacroDtosEntry` retired |
| `framework_surface_executor` (suite) | executor behavior: validation-first, unknown-adapter rejection, Vue parity, per-kind status |
