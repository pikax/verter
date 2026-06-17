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
| `framework/surface_store.rs` | Generic `FrameworkSurfaceStore<K,B>` + `FullKey<K>` + erasure (`ErasedFrameworkSurfaceStore`) + the `FrameworkSurfaceDtoBundle` marker. Strict same-generation + fact-rail warm reads. PROVISIONAL: this store (and `FrameworkScriptCaches` in `script_facts.rs`) live on the framework registry rows, NOT the single `ProjectTypeStore`, and have no in-flight collapse — U10 OWES consolidating `FrameworkSurfaceStore` onto `ProjectTypeStore`/`TypeInfoGraphResultDb` + adding true singleflight. |
| `framework/synth.rs` | `ComponentDefaultSynth` trait + parse-domain `ComponentDefaultSynthCtx` + `VueComponentDefaultSynth`. |
| `framework/script_facts.rs` | The resolved-validation half: `ActiveProviderIndex`-gated `resolve_script_facts`, the content-addressed candidate store, the fact-rail-validated resolved-fact store. |
| `framework/api_projector.rs` + `api_projectors/vue.rs` | `ComponentApiProjector` trait + the Vue leg delegating to `render_vue_public_api_legacy`. |
| `framework/virtual_file_naming_ts.rs` | Renders the byte-pinned TS mirror of the `VirtualFileNaming` column. |
| `verter_semantic/.../framework_facts.rs` | The syntax-capture half: `ScriptFactProvider` trait, closed `ScriptFactSyntaxGate`, candidate set, the zero-cost dispatcher param. |
| `typeinfo/framework_surface/{executor,plan,results,graph_export,vue_exec}.rs` | The executor entry, the closed plan/result vocabulary, the first `SemanticTypeGraph` encoder, and the relocated Vue resolution delegates. |
| `typeinfo/adapters/vue/adapter.rs` | `VueFrameworkAdapter` — plan/normalize only. |
| `verter_compiler/src/framework_common/carrier_compiler.rs` | The `CarrierCompiler` trait (compiler-domain: `adapter_id`, `parse`, `eval_source`, `compile_ide`, `template_data`) + its neutral I/O vocabulary (`ParseOptions`, `IdeCompileOptions`, `IdeOutput`, `CompileUnsupported`, `TemplateFacts`). NO script-fact method — script facts go through the one `ScriptFactProvider` seam. |
| `verter_compiler/src/framework_common/registry.rs` | `CarrierCompilerRegistry` (built once; the host's carrier parse dispatch looks the file's adapter compiler up here). |
| `verter_compiler/src/framework_common/ctx.rs` | `CarrierCompilerCtx` — the compiler-side blessed carrier downcast (D-m) + `receive_vue_carrier_token` (the compiler's sanctioned carrier-proof receipt site). |
| `verter_compiler/src/framework_common/vue_bridge.rs` | `VueCarrierCompiler` — the reference `CarrierCompiler`, delegating call-for-call to `parse_sfc` + `compile_from_parsed`; ZERO edits to any Vue parser/codegen module. |
| `verter_compiler/src/framework_common/sourcemap_e2e_helpers.rs` | Reusable (test-only) framework IDE sourcemap-correctness assertions every carrier vertical re-runs against its own `compile_ide` output. |

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

### Client framework manifest (de-Vue-forked client wiring)

A SECOND descriptor-generated, byte-pinned TS artifact —
`packages/language-shared/src/client-framework-manifest.generated.ts`,
rendered by `render_client_framework_manifest_ts` (in
`verter_session::framework::client_framework_manifest_ts`) from
`built_in_descriptors()` joined with the `verter_language` extension table
(`LanguageRegistry::adapter_module_extensions`) — is the SINGLE authority
for the VS Code extension + TS-plugin CLIENT wiring. Per registered carrier
adapter it records: framework id, carrier extension(s), adapter-module
extension(s) (rune modules), client language id(s), trigger language id(s),
the virtual-file naming suffixes, and the file-watch globs; plus the base
TS/JS surface and the flattened derived lists (activation / trigger /
carrier / watch). Byte-pinned by `client_framework_manifest_ts_freshness`
(regen path under `VERTER_UPDATE_CLIENT_FRAMEWORK_MANIFEST_TS`). The
extension consumes it through `packages/vue-vscode/src/frameworkWiring.ts`
(document selector, framework-carrier predicate, TS-plugin configure
trigger, watch globs) — NO per-framework `if (vue)`/`if (svelte)` branch,
and Svelte is FIRST-CLASS (the retired `verter.frameworks` opt-in gate is
gone). The extension's `package.json` framework wiring
(activation events, `contributes.languages`) MATCHES the manifest, pinned by
`client_framework_manifest_drives_extension_wiring` (scans the extension
source) + the TS specs `frameworkWiring.spec.ts` /
`packageManifestFramework.spec.ts`. Verter ships NO Svelte TextMate grammar
(it relies on the user's Svelte extension for syntax).

## The framework-surface executor

`VerterHost::resolve_framework_surface_with_audit(TypeInfoGraphRequest)` is
the SOLE audited entry for the `GRAPH_OPERATION_FRAMEWORK_SURFACES`
operation. It rides the EXISTING typeinfo graph envelope. The current
`FrameworkSurfacePayload`/embedded-`SemanticTypeGraph` shape is PROVISIONAL,
NOT final: the `S5.B11/B12 → U8` gate landed ahead of order, so U8 still
OWES the retag of `FrameworkSurfacePayload.graph` to a `TypeInfoGraphPayload`
carrier + the `SemanticTypeGraph.schema_version` bump (currently
`TYPEINFO_GRAPH_SCHEMA_VERSION = 4`) + reserving the old field per the
Typeinfo Wire Contract. Until U8 lands the wire stays pinned but is not a
permanent "no schema change" guarantee. Flow:

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
(`MacroPayload` / `PathProjection` / `ShallowSurface`
plus the Svelte arm `SvelteSurface` — its own `SvelteSurfaceSource` family,
NOT the Vue-coupled `MacroPayload` arm) — no `Custom` / `Raw` arm, no source
text, no OXC handles, no raw `SemanticQueryKey`s. `ResolvedOutcome` (Resolved / Partial / Unsupported /
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

## Compiler-side carrier substrate (`verter_compiler::framework_common`)

The compiler-domain mirror of the session registry: where the session
registry owns the carrier ACCESS token + the semantic legs, the
compiler-side `CarrierCompilerRegistry` owns the carrier COMPILER per
adapter. `CarrierCompiler` is one trait every carrier framework
implements, exposing EXACTLY four compiler-domain ops:

- `parse(source, opts) -> Arc<FrameworkParseArtifact>` — produce the
  framework-neutral artifact (infallible; tokenizers collect diagnostics
  inline).
- `eval_source(source, artifact) -> Arc<str>` — the position-preserving
  blank: script bytes at raw offsets, every other byte whitespace-blanked,
  line terminators preserved; output length == input length.
- `compile_ide(source, artifact, opts) -> Result<IdeOutput, CompileUnsupported>`
  — the rendered TSX/JSX IDE artifact. The adapter's IDE codegen owns its
  OWN `CodeTransform` (the single source of truth for generated-code
  edits) and returns the rendered output verbatim — NO borrowed caller
  `CodeTransform` (a shared one would be a second, coarse, non-authoritative
  map). An unsupported `CompileTarget` bit → typed `CompileUnsupported`
  (invariant 4), never a silent empty.
- `template_data(source, artifact) -> TemplateFacts` — neutral template
  facts (wraps `RawTemplateData`).
- `compile_bundle(source, artifact, opts: &RuntimeCompileOptions, alloc) ->
  Result<RuntimeCompileOutput, CompileUnsupported>` — the framework-neutral
  RUNTIME bundle. The host owns the cached-parse validity decision and hands
  over either the valid cached artifact or a fresh carrier parse of the merged
  source; the carrier owns the typed downcast + native compile and returns a
  NEUTRAL `RuntimeCompileOutput` (a `RuntimeMainModule` body + neutral
  script/template/style/custom blocks + scope id + optional IDE `tsx` (when
  `want_ide`) + optional template facts + neutral `diagnostics`). Vue uses
  `VerterCompileResult` INTERNALLY then re-expresses it neutrally
  (`vue_result_to_runtime_bundle`); it leaves `main.body_code = None` so the
  host assembles the `_sfc_main` module from the block fields
  (`assemble_vue_main_module`). A carrier that projects ONLY an IDE surface
  (Svelte today) returns a bundle with no runtime surface
  (`has_runtime_surface() == false`) carrying just the `tsx` — the host
  populates `CachedTsx` and emits NO `Main` virtual node. Framework-PRIVATE
  resolved inputs (Vue's `external_types` / `prop_constness` /
  `style_v_bind_vars`) ride OPAQUELY on `RuntimeCompileOptions.framework_extras`
  (`Arc<dyn Any>`, downcast to `vue_bridge::VueRuntimeCompileExtras`) so Vue's
  eager type-surface output type never enters the cross-framework contract.

There is NO `analyze_script_facts` method — script-fact extraction for
EVERY framework goes through the one `ScriptFactProvider` seam; the carrier
compiler is parse / eval / IDE / template / runtime-bundle only.

The host's carrier PARSE dispatch is rehoused through this registry: the
`FileLanguage::Framework` branch in `host_executor::execute_source` routes
EVERY carrier's parse through `parse::carrier_parse_snapshot` (which looks
the adapter's `CarrierCompiler` up in the process-wide registry — Vue via
`VueCarrierCompiler`) — a single dispatch path, no `is_vue` branch, no dual
Vue direct-parse path. A carrier row whose adapter has no registered
compiler is the typed unsupported-language state. `VueCarrierCompiler`
delegates call-for-call to `parse_sfc` + `compile_from_parsed` with ZERO
edits to any Vue parser/codegen module, so Vue compile output stays
byte-identical pre/post the rehousing (pinned by
`rehoused_carrier_dispatch_drives_compile_byte_identical_to_direct_compile`
and the unchanged `ide_virtual_output_for_fixture_sfc_is_byte_stable` hash).

The host's TEMPLATE-DATA ingestion is rehoused through this registry the
SAME way as the parse dispatch: `compute_template_analysis_if_missing`
/ `build_template_analysis` gate on `parse::file_language_has_template_data_compiler`
(does the file's carrier row have a registered compiler?) — NOT a hardcoded
`.vue` / `is_vue()` check — and extract through `parse::compile_template_data`,
which dispatches to the file's `CarrierCompiler::template_data(source, artifact)`.
Vue's bridge runs the META-target `compile_from_parsed` (for `referenced_bindings`
/ constness), byte-identical to the retired Vue-only `compile_vue_template_data`;
Svelte's walks the typed `ParsedSvelte` template tree. One registry-dispatched
path, no dual path/shim. The populated `RawTemplateData.components` reaches the
public `ComponentMetaBody.components` (`ComponentUsage`) — the same surface Vue
already published. Pinned by `template_data_ingestion_is_registry_dispatched`
(no `.vue` / `is_vue()` gate on the template-data path) and the public E2E
`svelte_component_meta_carries_template_usage_facts`.

The Svelte `template_data` producer (`verter_compiler/src/svelte/template_facts.rs`)
is TYPED-IR-ONLY: it walks the typed `ParsedSvelte` template tree (recurse
element children + block `children` + each clause's children, mirroring
`svelte_exec::collect_slot_elements`' walk shape), classifying child-component
usages BY KIND (`SvelteElementKind::Component`, `Special(Component)` dynamic-this,
`Special(SelfRef)`) and mapping attributes structurally — EVERY plain attribute
(including `on*`) → PROP (a plain `on*` attribute is a callback PROP, not a
template-usage event; the child's component-meta decides which props are callback
events), ONLY `Directive(On)` (the legacy `on:` directive) → neutral EVENT,
`Directive(Bind)` except
`bind:this` → neutral BINDING, `Directive(Let/Class/Style/Use/Transition/In/Out/
Animate)` + `bind:this` → SKIP, `Spread` → `has_spread`, child `{#snippet name}`
→ `slots_used`. Expression TEXT is span-sliced from the carrier source; there is
NO structural source scan and NO type lowering. The neutral per-usage
`bindings` / `events` fields (and the `ComponentBindingUsage` / `ComponentEventUsage`
proto messages 9/10, `COMPONENT_META_SCHEMA_VERSION` 3) are ADD-ONLY — Vue keeps
its two-way bindings in `v_models` and leaves these empty. Pinned by
`svelte_template_data_producer_is_typed_ir_only` and the carrier unit tests.

The compiler-side `CarrierCompilerCtx::carrier_for::<T>` is the third
blessed carrier-downcast home (D-m); `receive_vue_carrier_token` is the
compiler's sanctioned carrier-proof receipt site (the bridge reaches its
own `VueParseCarrier` back out of the type-erased artifact through it).

## Host RUNTIME-compile routing + the IDE-ensure path

The host's RUNTIME compile (`virtual_file_pipeline::compile_entry`) is routed
through the registry the SAME way as parse/template-data: it resolves the
carrier from `snapshot.framework_parse`'s `(adapter_id, language_id)`, KEEPS
the cached-parse validity decision (`can_use_cache` — src-merge + custom
delimiters/elements force a fresh carrier parse through the COUNTED chokepoint
`parse::parse_carrier_counted`), and calls `CarrierCompiler::compile_bundle`.
There is NO hardcoded `compile_sfc` / `compile_from_parsed` / `vue_parse` in
`compile_entry` and NO `is_vue` branch — pinned by the AST/`syn` guard
`compile_entry_routes_through_carrier_registry_not_hardcoded_vue` (catches
imports / aliases / renames / globs / calls of the forbidden producers, with a
negative self-test). Vue's runtime + IDE virtual-file outputs stay
byte-identical (golden-pinned by `svelte_compiler_block1::vue_*_byte_identical_*`,
`include_str!` goldens).

`get_virtual_file` is a thin projector over `ensure_compile_artifacts(canonical,
profile, demand: CompileDemand)` (`CompileDemand::{VirtualNode(kind), Ide}`):
the shared compile produces the WHOLE artifact set (every virtual node + the
IDE `CachedTsx`) in one pass, and the demand is checked AFTER the shared result
(warm-hit + cold). `ensure_ide_compiled(canonical, profile) -> Result<bool>`
is the EXPLICIT IDE-ensure path: it resolves through `CompileDemand::Ide`
(NEVER requests `VirtualNodeKind::Main`), so a Main-less carrier (Svelte)
populates its `CachedTsx` and succeeds without a runtime `Main`. `get_ide`
stays a PURE cached read (`peek_tsx`) — it NEVER computes on read. Both pinned
by the static guards `get_ide_is_a_pure_cached_read_no_compute` and
`ensure_ide_compiled_never_requests_virtual_node_main`. Exposed on WASM + NAPI
as `ensureIdeCompiled(canonicalId, profile?)` (binding-parity tested).

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

## Carrier-generic LSP routing (no hardcoded Vue gate)

The `verter_lsp` feature / server ROUTING layer is carrier-generic: every
framework CARRIER (`.vue`, `.svelte`, …) reaches the SAME LSP features. A
routing decision NEVER hardcodes a Vue-only gate — no `.is_vue()` call, no
`ends_with(".vue")` / `strip_suffix(".vue")` / `trim_end_matches(".vue")`
suffix check, and no hardcoded carrier provider literal (`.vue.ts` /
`.vue.tsx` / `.vue.jsx`) — in executable routing code. Such a gate keeps
attracting Vue-only behaviour and silently strands every other carrier at
less than full parity. Route through the shared substrate instead:

- `file_language.is_framework_carrier()` when a `FileLanguage` is in hand,
- `crate::server::carrier_language_for(path).is_some()` for URI / canonical
  routing (the LSP-local registry-backed classifier),
- `crate::server::is_default_export_component_carrier(path)` for the
  default-export / component-target navigation gates (every carrier shares
  default-export component semantics — none is Vue-intrinsic),
- `verter_workspace::path_is_carrier` / `strip_carrier_extension` for pure
  path / stem helpers (registry-owned, longest-suffix-first),
- `verter_workspace::carrier_ide_provider_path` / `carrier_api_provider_path`
  (or the resolver `provider_*_for_source` / `source_id_from_provider_id`
  helpers) for the provider virtual-file path — the IDE / API suffix is
  carrier-owned, so `Foo.svelte` projects `Foo.svelte.tsx` / `Foo.svelte.ts`,
  never a hardcoded `.vue.ts`.

The custom project-overview wire shape is carrier-neutral: the stats field is
`totalComponentFiles` and the per-file kind discriminant is `"component"`
(not `"vue"`) — `.svelte` is counted in the component graph.

The carrier-generic-but-Vue-named routing / provider-sync / watcher /
position-mapper primitives are named `carrier_*` (e.g. `carrier_uri_from_ide_path`,
`source_id_from_provider_carrier_path`, `carrier_sync_state_for_source`,
`open_unresolved_carrier_state`, `prepare_carrier_provider_sync_transition`,
`resync_background_carrier_file`, `sync_pending_carrier_provider_file`,
`sync_imported_carrier_apis`, `imported_carrier_priority_ids`,
`carrier_resync_ids` / `carrier_delete_ids` (the watcher queues),
`carrier_position_to_tsx_offset[_validated]`, `tsx_range_to_carrier[_range]`,
`carrier_line_index`, `PositionMapper::carrier_to_tsx` / `tsx_to_carrier`,
`direct_import_binding_hover_target`) so the names stop attracting Vue-only
behaviour; no re-export shims. The plain-script (non-carrier) provider-sync
family is `non_carrier_*` (`prepare_non_carrier_provider_sync`,
`PreparedNonCarrierProviderSync`, `rewrite_non_carrier_source_with_resolver`,
`sync_non_carrier_file_to_provider`, `resync_non_carrier_file`,
`sync_pending_non_carrier_provider_file`,
`prepare_non_carrier_provider_sync_transition`) — these key on plain
`.ts/.tsx/.js/.jsx` scripts via `provider_id_for_source` / `is_plain_source_file`
(which return `None`/`false` for ALL carriers), so the old `non_vue` name was
misleading (a `.svelte` carrier is "non-Vue" but must NOT route here).

The genuinely Vue-INTRINSIC names are KEPT: the Vue-SFC `@event`/`:prop`
attribute mappers (`jsx_prop_to_vue_attr`, `extract_vue_attr_label`,
`replace_primary_label_with_vue_attr`, `vue_attr`, `vue_label`), the Vue hover
kind label (`vue_kind_label`), the Vue runtime-API classification
(`VueApiClassification` / `VueApiCallSite` / the `vue_api` binding field /
`vue_api_calls` / `vue_api_hover_at_offset`), the Vue built-in component tags
(`VUE_BUILTINS`), and `FileLanguage::is_vue()`.

The routing rule is pinned by TWO static guards, both scanning the same non-test
production routing source (`features/`, `server/`, `documents/`, `tsgo/`, plus
`server_utils.rs` / `background_drain.rs` / `provider_sync.rs` /
`sync_coordinator.rs` / `workspace_scanner.rs`), both stripping comments +
`#[cfg(test)]` blocks + `*_tests.rs` files and carving out
`extract_component.rs` (Vue-SFC code-action codegen, not carrier routing). They
differ on STRING literals: the literal-GATE guard KEEPS string contents (its
gate forms — `== "vue"`, `starts_with("vue")`, `ends_with(".vue")`, `.vue.ts` —
ARE string literals, so it cannot blank them; it is string-aware only to avoid
treating a `//` inside a literal as a comment), while the NAMING guard BLANKS
string literals (a `vue`/`Vue` word inside the `"vue"` npm specifier or a log
message is not a routed identifier):

- `carrier_lsp_routing_has_no_hardcoded_vue_gate`
  (`crates/verter_lsp/tests/carrier_routing_no_vue_gate.rs`) — bans executable
  `.vue` / `"vue"` GATES: `.is_vue(`, the `.vue`-suffix classifiers
  (`ends_with` / `strip_suffix` / `trim_end_matches` / `starts_with` /
  `contains` against `".vue"`), the `.vue` / `"vue"` equality + `matches!`
  gates, the bare `"vue"` LANGUAGE-ID classifiers (`contains` / `starts_with` /
  `ends_with` / `strip_prefix` / `strip_suffix` / `trim_*_matches` against the
  bare `"vue"` — e.g. `language_id.starts_with("vue")`), the hardcoded carrier
  provider literals (`.vue.ts` / `.vue.tsx` / `.vue.jsx`), and the `.vue`
  provider/routing-path builders. It MASKS ONLY the `.server.vue` / `.client.vue`
  SSR needles (needle-narrow — a bare `.vue` gate BESIDE an SSR check still
  flags). Self-discriminating: its detector FLAGS the pre-change `.vue`-gated
  shapes and PASSES the carrier-generic post-change shapes.
- `carrier_routing_has_no_vue_named_generic_primitive`
  (`crates/verter_lsp/tests/carrier_routing_no_vue_named_primitive.rs`) — the
  NAMING half that ends the whack-a-mole: bans ANY `vue`/`Vue`-substring
  identifier in the scanned production code outside the Vue-intrinsic allowlist
  (the attr mappers, hover label, runtime-API classification, `VUE_BUILTINS`,
  `is_vue` above). Multi-line-raw-string-aware so a test SFC fixture's braces
  never tear the `#[cfg(test)]` scope.

NOT allowlisted by either guard: `.vue` / Vue-named primitives in definition /
navigation / component-resolution, workspace symbols, component import / drop,
watcher carrier routing, provider-path reverse-mapping, position mapping, or
barrel / provider sync — exactly the categories that must be carrier-generic.

### Rune-module own-buffer LSP path (`.svelte.ts` / `.svelte.js`)

A standalone rune module is a NON-component carrier — NOT in `carrier_extensions()`,
covered instead by the dedicated descriptor-derived adapter-module watch glob
(`all_adapter_module_extensions()` → `**/*.{svelte.ts,svelte.js}`). Its provider
buffer is its OWN canonical path serving `<rune prelude> + <rewritten module
bytes>`. The LSP own-buffer path:

- `DocumentState.projection: Option<DocumentProviderProjection>` (a clean
  cutover from `position_mapper`) carries either `CarrierIde { mapper:
  PositionMapper }` or `SelfFile { mapper: SelfFileProviderMapper }`. The
  `provider_path` is DERIVED (carrier IDE path / canonical id), never stored.
- `SelfFileProviderMapper` (in `documents/provider_projection.rs`) is line-only
  + rewrite-aware: source→provider shifts the line by `prelude_line_count` and
  the column by the per-line import-specifier rewrite delta (DROP inside a
  rewritten specifier); provider→source DROPS the prelude region (never clamps)
  and undoes the rewrite delta. The rewrite segments come from the SAME
  `compute_specifier_replacements` the background non-carrier sync uses.
- `ProviderPositionMapper` (the `SourceMap`/`SelfFile` enum) exposes the three
  ops the `tsgo::merge` helpers use (`tsx_to_carrier` / `carrier_to_tsx` /
  `tsx_range_to_carrier`), so every feature maps positions uniformly.
- `provider_projection_context` is the ONE generalized query context serving
  BOTH projections (no parallel rune path); `type_provider_context` /
  `ide_context` route through it.
- `did_open` syncs the rune module's self-file Shadow provider state (keyed at
  the canonical path) so it is queryable BEFORE resolver ownership (does NOT
  depend on `non_carrier_sync_state_for_source`); `did_close` closes + removes
  it via an explicit non-carrier branch (the carrier did_close gate never fires
  for a non-carrier). The `did_change_watched_files` batch classifies a closed
  rune-module edit through an EXPLICIT adapter-module branch and resyncs it via
  the non-carrier resync impl.
- Follow-up (out of S2c scope): rename + code-actions for the rune-module own
  buffer (they need workspace-edit mapping through the same mapper first).

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
| `client_framework_manifest_ts_freshness` | the generated client framework manifest is byte-equal to the rendered descriptor registry |
| `client_framework_manifest_drives_extension_wiring` | the extension wiring (activation / document selector / trigger / watch) derives from the manifest; Svelte ungated; no per-framework client fork |
| `vue_relocation_no_shim` | no re-export shim for relocated Vue resolution; deleted files stay deleted |
| `retired_symbols_absent_from_production_source` | `VueShallowMetadataStore` / `VueMacroDtoKey` / `VueMacroDtos` / `VueMacroDtosEntry` retired |
| `framework_surface_executor` (suite) | executor behavior: validation-first, unknown-adapter rejection, Vue parity, per-kind status |
| `carrier_lsp_routing_has_no_hardcoded_vue_gate` | no executable `.vue`/`"vue"` GATE in non-test feature/server/documents/tsgo LSP routing: `.is_vue(`, `.vue`-suffix classifiers, `.vue`/`"vue"` equality + `matches!`, the bare `"vue"`-prefix language-id classifiers (`language_id.starts_with("vue")` etc.), `.vue.ts/.tsx/.jsx` literals, `.vue` provider-path builders — outside the needle-narrow SSR-convention (`.server.vue`/`.client.vue`) / `is_svelte()` / `extract_component.rs` allowlist |
| `carrier_routing_has_no_vue_named_generic_primitive` | the NAMING half: no `vue`/`Vue`-substring IDENTIFIER for a carrier-generic routing/provider-sync/watcher/position-mapper or plain-script (`non_carrier_*`) primitive in the scanned production modules, outside the narrow Vue-intrinsic allowlist (`@event`/`:prop` attr mappers, `vue_kind_label`, the `vue_api*` runtime-API classification, `VUE_BUILTINS`, `is_vue`); ends the whack-a-mole |
| `framework_codegen_uses_code_transform` | the carrier-compiler IDE codegen path delegates to the CodeTransform-backed pipeline and never post-hoc string-munges built output (+ negative self-test) |
| `carrier_descriptors_have_compilers` | every carrier-bearing session descriptor has a registered `CarrierCompiler` (Vue-through-the-bridge) |
| `framework_known_bug_ledger_bijection` | 1:1 between framework known-bug ledger entries and their `#[ignore]`d characterizing tests (empty ledger ⇒ trivially green; non-empty enforcement self-tested) |
| `rehoused_carrier_dispatch_drives_compile_byte_identical_to_direct_compile` | the rehoused carrier parse dispatch drives Vue `compile()` byte-identical to the compiler's direct `compile()` |
