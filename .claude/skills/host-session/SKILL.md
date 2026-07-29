---
name: host-session
description: "LSP host integration: TypeProvider (TSGO/tsserver), workspace management, async scheduler, SyncCoordinator, ownership lifecycle"
---

# Host & Session

## Project-Global Cache on `VerterHost` (post-rewrite)

`VerterHost` owns one `Arc<ProjectTypeStore>` per loaded project, exposed via `.project_type_store()` — the single shared cache graph for component-meta and cross-file type resolution:

- `FileArtifactStore` — canonical post-parse artifacts.
- `AnalysisReadyDb` — scope-parameterised analysis augmentation.
- `RouteDb` (rehomed) — barrel / route surface cache.
- `OwnerImportSurfaceDb` — direct-owner-imports cache. Reached via `VerterHost::owner_import_surface` / `resolve_owner_direct_import`.
- `ComponentMetaResultDb<ComponentMetaAnalysis>` — final component-meta payload cache consulted by `get_component_meta` before any cold work.
- `SemanticGraphStore` — host-owned semantic-query memo, dispatched through `ProjectSemanticDispatch::execute`. The canonical lazy semantic layer and sole authority for reusable type-resolution work. Two parallel memos:
  - **Node memo** (mode-erased `FamilyKey` → `FamilySlots`) for single-node queries (`ResolveDecl`, `Instantiate`, `KeyOf`, `MappedType`, `Conditional`, `ProjectPath`, `TypeOf`, `NormalizeUnion`, `NormalizeIntersection`, `ResolvedNamedType`).
  - **Relation memo** (keyed by full-identity `RelateMemoKey` — source / target / relation kind / policy / source freshness / inference context / env+substitution+projection-reduction context) for `Relate` judgements. `RelationResult` is `{ Assignable { bindings }, NotAssignable, Unknown }` — all three cache-with-fence.
  - Canonical node variants include `SemanticNodeData::Function { params, return_type, type_parameters }` (class/interface lower to `Object` with heritage merged).
- `IntrinsicRegistry` — SDK-intrinsic dispatch table.
- `ProjectTypeStoreCounters` — per-layer live / stale / in-flight counters.

**Own-canonical drain** runs on every `upsert` for the upserted canonical itself: `resolver.runtime.evict_canonical(&canonical_id)` + `project_type_store.evict_canonical(&canonical_id)` + `resolved_type_cache().clear()` — drained together so a file-content change cannot leave one cache authority stale for that file. NO reverse-dependent cascade: an `upsert` never iterates `reverse_deps_for` to drain dependents. Cross-file consumers revalidate lazily on read via their own `fact_dep_signature` checks. (Retained until query-identity caches self-version-root a same-canonical content edit.) Workspace-shape changes (tsconfig / SDK / project-graph) call `bump_project_generation_and_evict`, clearing route-sensitive layers (`OwnerImportSurfaceDb`, `ComponentMetaResultDb`, `SemanticGraphStore`) atomically.

Host view: resolver-path helpers receive `&HostStoreView` directly as result-DB fence authority; `IndexedReady` is the single canonical post-parse artifact (former `ModuleFactsDb` deleted). Validated-cache writes record a `ReadSetSignature.facts` fact signature; warm hits revalidate it against the live `StoreView` before returning. Full store-view contract: "Host Store View" + "Store-View Token, Lane Identity, and Singleflight" below.

**Resolver-context seal:** resolver-path code does NOT take `&VerterHost` directly. It takes `ctx: &'a dyn ResolverContext` — a `pub(crate)` sealed super-trait at `crates/verter_session/src/resolver_core/resolver_context.rs`. Only `VerterHost` implements `ResolverContext` (`sealed::Sealed` marker closed at trait definition). Guard `no_concrete_verter_host_in_seal_scope` mechanically forbids re-introducing `&VerterHost` parameters under the resolver_core/meta_resolve/host_manage/component_meta_query_engine seal scope. New trait-surface methods are an architectural decision; widen with care.

## Vue Macro Codegen Producer

`typeinfo/vue_macro_codegen.rs` is the sole semantic producer for compiler-facing
Vue macro DTOs. One invocation inventories one already-indexed SFC under one
request-bound `ResolverContext` and fulfills `Runtime`, `Tsc`, or
`RuntimeAndTsc` demand. Runtime and TSC bundles are independent. Each entry is
joined to compiler syntax by stable `syntax_index`; semantic values come from
`ProjectSemanticDispatch` and TypeInfo projection, never from parser type
expansion.

The output is request-local and is not retained as an aggregate graph-id cache.
Underlying semantic queries keep their canonical memo/singleflight behavior.
The producer submits exactly one interactive scoped cache-node job per
`(canonical SFC, exact demand, session)` request. The semantic key is
content-free; resolver epoch and external-validity fingerprint live only in the
job's input pin, so an edit preserves the key while moving the pin. Every macro,
member, owner, runtime classifier, and TSC materialization stays inside that one
scheduled closure. The scoped rendezvous disappears at terminal state and is
not a second cache authority.

The producer records one fact footprint, returns typed
completeness/cacheability, and reports the sorted transitive canonicals observed
by that request. Cancellation returns only typed `Partial(Cancelled)` entries,
marks the result non-cacheable, and never publishes the cancelled aggregate; a
live dedup sibling may still complete and publish to that sibling. Compile entry
points call the producer once per SFC/request for the target's combined demand,
pass `output.compiler_input()` to the compiler, and unconditionally replace the
semantic transitive-dependency axis with the returned canonical set. Public-API
rendering requests one TSC bundle; runtime rendering requests one runtime
bundle. Audited compilation invokes the producer inside the request observer
scope so its semantic reads are attributed to the same audit record.

Vue custom-element script policy is an explicit compile-profile axis:
`CompileProfile.custom_element` (host/NAPI `customElement`) defaults to `false`,
participates in the derived profile hash, and threads through
`RuntimeCompileOptions` to `CodegenOptions.custom_element`. The batch-render
lane carries the same axis as required
`CompileBatchRenderProfile.custom_element`; callers must set it explicitly.
Never infer it from template `custom_elements`, whose only responsibility is
custom-tag parsing. The standard dev/prod profiles remain non-custom-element
unless the caller selects this axis.

## Language Server Architecture

The LSP is a standalone Rust binary (`verter-lsp`) communicating with VS Code over stdio.

```
main.rs (stdio transport + CLI args + provider selection)
    |
server/ (LSP message loop, request dispatch — split post-Phase-11e into mod.rs + 8 siblings: handler_guard, provider_state, component_resolve, sync_orchestration, custom_methods, lifecycle, aux_features, nav_features)
    |
documents/       -> Document tracking and synchronization
features/        -> LSP feature handlers (see table below)
analysis/        -> Static analysis integration
css/             -> CSS-specific language features
tsgo/            -> TSGO type provider integration (LSP protocol)
tsserver/        -> tsserver type provider integration (JSON protocol)
capabilities.rs  -> Server capability registration
config.rs        -> Server configuration, ProjectConfig, ProjectRegistry, vite alias discovery
workspace_scanner.rs -> Async priority-based workspace file scanner
sync_coordinator.rs  -> Debounced type provider sync during typing
```

### Per-Project Configuration (`config.rs`)

`ProjectRegistry` groups per-project config for multi-root workspaces. Each `ProjectConfig` has: root path, `TsConfigPathResolver`, `ResolvedLintConfig`, `Linter` instance, optional `vite_config_path`/`vite_config_deps`. Tsconfig-backed projects use only tsconfig paths; fallback projects get Vite aliases via OXC static analysis (`vite_config.rs`) or trusted Node.js execution.

### TypeProvider Trait (`tsgo/traits.rs`)

Both TSGO and tsserver implement `TypeProvider`. Methods: hover, completions, diagnostics, definition, references, rename, signature help, code actions, semantic tokens, highlights, inlay hints, open/update/close file, shutdown. Object-safe (`dyn TypeProvider`) so the server is backend-agnostic.

### TSGO Module (`tsgo/`)

| File              | Purpose                                                                                        |
| ----------------- | ---------------------------------------------------------------------------------------------- |
| `ipc.rs`          | LSP client: Content-Length framing, JSON-RPC request/response correlation                      |
| `traits.rs`       | `TypeProvider` trait definition + `TypeProviderError`                                          |
| `protocol.rs`     | Response types: `CompletionResult`, `HoverInfo`, `TypeDiagnostic`, etc.                        |
| `resilient.rs`    | `ResilientTypeProvider`: crash/silence detection via `Notify`, auto-restart with attempt-bounded respawn, file cache replay |
| `project_sync.rs` | `ProjectSync`: batches `open_tsx`/`sync_tsx`/`close_tsx` calls to the provider                 |
| `merge.rs`        | Merges TSGO diagnostics/completions with verter's own results                                  |
| `mock.rs`         | Mock provider for integration tests                                                            |

### tsserver Module (`tsserver/`)

| File                | Purpose                                                                                                                                           |
| ------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------- |
| `mod.rs`            | `find_tsserver()`, `find_node()` re-exports from `verter_type_runtime::discovery`                                                                 |
| `project_router.rs` | `ProjectTsserverProvider` — the PRODUCTION tsserver provider: per-owning-project engine routing + `probe_workspace_tsserver` route selection      |
| `ipc.rs`            | `TsserverTypeProvider`: newline-delimited JSON transport, position conversion (byte offset <-> 1-based line/offset), all `TypeProvider` methods    |
| `resilient.rs`      | `ResilientTsserverProvider`: same crash/restart pattern as TSGO resilient wrapper                                                                 |

### Per-Project tsserver Routing (`tsserver/project_router.rs`)

The tsserver tier is served by `ProjectTsserverProvider`, NOT by one workspace-level engine. A pnpm monorepo routinely installs no TypeScript at the workspace root while each package pins its own (5.8 next to 6.0); one workspace-root resolution walks past every real install onto whatever ancestor or configured `tsdk` answers — including a library-less copy whose Program has NO default libs, so valid code reports `Cannot find name 'Math'`.

- **Engine identity** is `(owning tsconfig, real canonical `tsserver.js`)`. Two projects that resolve the same install share one process; two projects on different TypeScript versions never do.
- **Every operation is project-bound.** A provider path maps to its authored carrier source (`classify_carrier_companion`, or the publish path's registered route), then through `resolve_carrier` (`PresentSnapshotAuthoritative`) → `ProjectBinding` → `TsserverEngineBackend::ensure_project` → `BoundProject`. Discovery runs from the OWNING project's directory (`resolve_tsserver(tsdk, Some(project_dir))`), so the pnpm `node_modules/typescript` symlink canonicalizes to the real `.pnpm/typescript@<v>` install (load-bearing: tsserver finds `lib.*.d.ts` relative to its own script path).
- **Fail-closed per project.** `NotReady` / `NoProject` / `Ambiguous` / an unresolvable or TS7+ TypeScript is a DISTINCT refusal for THAT project carrying discovery's actionable install message. It never poisons a sibling project and never borrows a sibling's engine.
- **Lazy + singleflight.** Construction starts no process. One `tokio::sync::OnceCell` per engine identity collapses concurrent cold demands onto one spawn; a failed spawn leaves the cell uninitialized so the next demand retries. `shutdown` / `resync_open_files` / `update_workspace_folders` fan out to every started engine.
- **Resolution caching.** Per-project engine resolution is cached under a published-snapshot generation fence (success AND refusal), so hover/completion never repeats the ancestor walk, the `read_dir` of the install's `lib/`, or discovery's `npm root -g` fallback. A project-graph republish re-resolves; a bare `node_modules` mutation that publishes no snapshot still needs a reload.
- **`child_pid()`** returns the first started engine's PID. `$/verter/typeProviderStarted` carries exactly one PID (a single-engine wire affordance); orphan containment does not depend on it — every spawned tsserver arms its own process-group `TreeKill` and registers in the process-wide engine-tree table the client-death monitor terminates in full.

### Provider Selection (`main.rs`)

CLI arg `--type-provider=auto|shared-tsgo|tsgo|tsserver|extension|off` (from VS Code `verter.typeProvider` setting):

- **auto**: the managed fallback is CAPABILITY-driven (`probe_managed_engine` + `choose_managed_engine`): a supported tsgo (stable `>=7.0.2, <7.1.0`) wins; otherwise the project tsserver router serves; `None` only when neither is obtainable. The lazy managed activation applies the same order at demand time (a candidate that fails validation — e.g. `VERTER_TSGO_BIN`/`PATH` naming a TypeScript 5.x/6.x `tsc` — falls through to tsserver, and the client is sent an updated `$/verter/typeProviderStatus`).
- **tsgo**: explicit managed tsgo; falls back to the project tsserver router when no supported tsgo validates, `None` only when tsserver is also unavailable.
- **tsserver**: explicit tsserver; a workspace whose ONLY resolvable TypeScript is TS7+ reclassifies to the managed tsgo route (the native family is never served over the Node protocol).
- **off**: verter-only mode

One provider SHAPE runs at a time; on the tsserver tier that shape owns N engines (one per owning configured project). The route DECISION is taken at startup, before any published project graph exists, from `probe_workspace_tsserver` + `tsserver_route_decision` (`main.rs`): a filesystem-only sweep of every configured project under the root answering "can ANY of them obtain a servable install?". `native_family_only` (at least one project resolved, and every resolved install is TS7+) is the reclassification signal; `servable` supplies `ManagedEngineFacts.tsserver`. Provider PID is sent to the extension via `$/verter/typeProviderStarted` for orphan cleanup.

**Topology**: `TypeProviderTopology::ProjectTsserver` (wire `project-tsserver`) — "Node tsservers Verter spawned and owns, one per owning configured project, each from that project's own TypeScript". It replaced the single-engine `WorkspaceTsserver` / `workspace-tsserver`; the TS union in `packages/language-shared/src/notifications.ts` and the `packages/vue-vscode` status bar carry the same token.

**tsserver serving tiers** (floors live in ONE place — `verter_type_runtime::discovery`: `TSSERVER_SUPPORTED_FLOOR`/`TSSERVER_CURRENT_MAJOR`/`tsserver_serving_tier`/`tsserver_serving_advisory`): TypeScript `>=6` is served silently; `>=5.8, <6` is served WITH a one-time upgrade warning (TypeScript 6 or 7; 7 enables the native tsgo engine) delivered via `LspConfig.type_provider_advisory` and shown in `initialized()`; `<5.8` is served best-effort with a below-floor warning. With per-project engines the advisory is computed from the LOWEST version that will actually serve (`WorkspaceTsserverProbe::lowest_servable_version`), so a workspace where one package still runs 5.8 is advised even when another runs 6.0. A TypeScript 5.x/6.x `tsc` probed during tsgo resolution classifies as `RejectionReason::NotTsgoEngine` (the tsserver family), never as a below-floor tsgo.

**Discovery tiers** (`verter_type_runtime::discovery::resolve_tsserver`): `ProjectLocal` (walk UP from the OWNING project dir) → `ConfiguredTsdk` → `Global` (`npm root -g`). There is no bundled tier. Every candidate is canonicalized through package-manager symlinks and must carry at least one sibling `lib.*.d.ts`; a library-less install is REFUSED and the next explicit tier considered. When nothing is usable, `TsserverDiscoveryError` preserves every refusal and names the action (`npm install -D typescript`, or the `typescript.tsdk` setting).

### LSP Features (`features/`)

| Module               | LSP Method                          | Description                                                              |
| -------------------- | ----------------------------------- | ------------------------------------------------------------------------ |
| `completion`         | `textDocument/completion`           | Auto-completions (components, props, CSS classes)                        |
| `definition`         | `textDocument/definition`           | Go-to-definition: bindings, imports, CSS <-> template, DOM query selectors |
| `hover`              | `textDocument/hover`                | Hover info: types, CSS rules on elements, elements on selectors          |
| `diagnostics`        | `textDocument/publishDiagnostics`   | Script/template diagnostics                                              |
| `css_diagnostics`    | (custom)                            | Unused scoped CSS hints, cross-file cascade detection                    |
| `inlay_hints`        | `textDocument/inlayHint`            | DOM query -> matched element, `useTemplateRef` -> matched ref            |
| `folding_range`      | `textDocument/foldingRange`         | SFC block + template element folding                                     |
| `linked_editing`     | `textDocument/linkedEditingRange`   | Rename matching open/close HTML tags                                     |
| `rename`             | `textDocument/rename`               | Symbol renaming                                                          |
| `references`         | `textDocument/references`           | Find all references                                                      |
| `document_symbol`    | `textDocument/documentSymbol`       | Document outline                                                         |
| `document_highlight` | `textDocument/documentHighlight`    | Highlight same symbols                                                   |
| `code_lens`          | `textDocument/codeLens`             | Code lens annotations                                                    |
| `color_info`         | `textDocument/documentColor`        | CSS color picker                                                         |
| `formatting`         | `textDocument/formatting`           | Document formatting                                                      |
| `organize_imports`   | `source.organizeImports`            | Import organization                                                      |
| `extract_component`  | (code action)                       | Extract selection to new component                                       |
| `call_hierarchy`     | `textDocument/prepareCallHierarchy` | Call hierarchy navigation                                                |
| `workspace_symbol`   | `workspace/symbol`                  | Project-wide symbol search                                               |
| `document_link`      | `textDocument/documentLink`         | Clickable links in source                                                |
| `document_drop_edit` | `textDocument/onDropEdit`           | Drag-and-drop editing                                                    |

### LSP Feature Flow

```
Request (stdio) -> server/mod.rs -> Find document in host cache -> Feature handler -> Response (stdio)
```

## TypeProvider Architecture

The LSP delegates TypeScript type checking to an external **TypeProvider** process. Two backends:

| Backend      | Binary             | Protocol                                   | Use Case                             |
| ------------ | ------------------ | ------------------------------------------ | ------------------------------------ |
| **TSGO**     | `tsgo` (Go binary) | LSP over stdio (Content-Length + JSON-RPC) | Fast, native TS checking (preview)   |
| **tsserver** | `node tsserver.js` | Newline-delimited JSON over stdio          | Workspace TS version, plugin support |

**tsserver kind mapping**: `parse_tsserver_completion()` in `tsserver/ipc.rs` maps tsserver's `ScriptElementKind` strings to LSP `CompletionItemKind`. MUST match VS Code's `MyCompletionItem.convertKind()` exactly. Test coverage: `test_parse_tsserver_completion_kinds_match_vscode`. Sync with VS Code source when updating TypeScript dependencies.

**Store-backed carrier refresh**: the plugin/store owns generated carrier membership and snapshots; protocol opens never carry generated bytes. `TsserverTypeProvider::register_carrier_member` hydrates local routing/position state and tracks the authored source in the explicit active working set. The first activation in a cold configured project uses one transient contentless companion open to instantiate the project/plugin, then immediately closes it; each editor-active authored source remains contentlessly open as the durable configured-project owner, matching the lifecycle official Volar/Svelte plugins receive from the editor. Workspace-discovered sources stay closed and enter project-scoped `getExternalFiles` lazily. Repeated activation is a no-op. Publishing advances the plugin's monotonic `carrierStoreRefreshToken`; a detached coalescing actor submits one background batch only after interactive traffic has been quiet for the grace window. That batch performs the constant-size `configurePlugin` refresh and one response-bearing no-op `configure` host-turn fence under a single background admission, so the project/plugin mutation queued for the next Node event-loop turn is visible before later interactive frames without paying the idle grace twice. An interactive carrier activation supersedes a running background refresh and republishes the newest working-set generation immediately. Any background refresh preempted by interactive traffic is requeued after the interactive lane becomes idle; retry is not conditional on the urgent-generation counter, because ordinary hover/completion may preempt without advancing it. The plugin compares manifest identities, reloads only changed source ScriptInfos, clears only the owning project's resolution cache, and reconciles ready authored-source roots through TypeScript's public `Project.addRoot` / `Project.removeFile` APIs. No generated file list, `updateOpen` payload, private whole-project filename reload, or per-file `projectInfo` probe is sent. Interactive requests preempt queued/running background diagnostics and refresh work through tsserver's cancellation pipe; creation of that out-of-band channel is a startup invariant, never a best-effort degradation to an unpreemptible session. Cancellation files are retained by exact request sequence until the matching response/`requestCompleted` acknowledgement (never reaped by age/count while an unbounded request may still be queued). Project-loading events suspend false hang strikes while retaining the absolute-silence backstop. The silence window begins at the later of the last engine message and the start of the current non-empty pending interval, so a request after a long idle receives the full health allowance.

**`@verter/types` resolution**: generated carriers retain the public `@verter/types` import. A project-installed or project-host-resolved package is authoritative. When resolution misses, the tsserver plugin serves its bundled declaration at a virtual package path; managed tsgo rewrites only the provider buffer to an adjacent virtual declaration overlay. Neither fallback writes into the user's `node_modules`. The tsgo carrier and overlay are serialized as one lifecycle unit: load/open/update semantics match, the dependency is published before a rewritten carrier, a newly created dependency is rolled back if carrier publication fails, transition to an installed package closes the old overlay only after the unrewritten carrier succeeds, and close cleans both.

**No-silent-empty provider recovery (hover / definition / typeDefinition)**: a FAILED provider query (`Err` from router bring-up, transport, engine restart, torn publish) never degrades to a vanishing tooltip or a silently dead CTRL+CLICK. All three carrier paths share ONE bounded-recovery owner (`server/provider_recovery.rs`, provider-neutral, above the per-route trait): resync the current file, recapture the request surface, and retry exactly once against it, validating and merging against the retry surface. The retry is IDENTITY-FENCED — it runs only when the recaptured surface's carrier `source_hash` equals the initial capture's, because a concurrent edit can put a different token at the same coordinates and a cross-revision retry would return a coherent-but-wrong result for the original request; on drift the recovery fails closed to the native result. Recovery is attempt-bounded, not wall-clock-bounded; a persistent failure fails closed after the retry, a legitimate empty (`Ok(None)` / `Ok(vec![])`) is not retried, and the resync is a current-file repair on the error path — never a dependency-publication join.

**Key modules** (`crates/verter_lsp/src/`):

- `tsgo/` -- TSGO integration (LSP client, resilient wrapper, project sync)
- `tsserver/mod.rs` -- `find_tsserver()` / `find_node()` re-exports from `verter_type_runtime::discovery`
- `tsserver/project_router.rs` -- `ProjectTsserverProvider` (per-owning-project engines), `probe_workspace_tsserver`
- `tsserver/ipc.rs` -- `TsserverTypeProvider`, newline-delimited JSON transport, position conversion
- `tsserver/resilient.rs` -- `ResilientTsserverProvider` (crash detection + auto-restart)
- `workspace_scanner.rs` -- Async background workspace scanner with priority-based file loading

### Background File Sync

During `initialized()`, the LSP spawns a priority-aware `WorkspaceScanner`. Filesystem-backed tsserver continues to resolve real `.ts`/`.tsx`/`.js`/`.jsx` and `node_modules` from disk. Framework carriers are compiled on the background lane and published by authored source identity into the durable plugin store; after the carrier pass, one coalesced refresh advances metadata without making every workspace carrier a Program root. No generated file is opened over the tsserver protocol. Before every carrier unit the scanner yields to active LSP handlers, with a bounded background-deferral interval so continuous editor traffic cannot starve project-wide warmup. TSGO retains its explicit eager project-input path for carrier and plain-source materialization. Verter semantic/type-info caches remain a separate host concern; they are not serialized into either TypeScript engine or used as a substitute for its project graph.

### Ordinary Carrier Import → Public-API Surface

An ordinary module import of a carrier (`import C from "./Comp.vue"` from a plain
`.ts`/`.tsx`, or from a generated carrier buffer) resolves to the
descriptor-generated IMPORT surface (`{carrier}.verter.ts`) or to NOTHING. The IDE
`CarrierIde` companion (`{carrier}.vue.tsx` / `.jsx`) is the editor's own
diagnostic root and is never a redirect target: handing it to a consuming module
makes that module's project require `jsx` (TypeScript reports TS6142 `--jsx is not
set` for a `.tsx` resolution when `options.jsx` is unset) and exposes template
lowering rather than the component's public contract.

Two owners, deliberately split:

- **WHICH surface** — `@verter/language-shared` carrier policy
  (`carrier/policy.ts::resolveCarrierImportTarget`). It derives the candidate
  path from the descriptor columns in the byte-pinned
  `virtual-file-naming.generated.ts` mirror, requires the owning project to own a
  `CarrierApi` row whose `provider_uri` IS that path (the reader matches a query
  against either `source_uri` or `provider_uri`, so identity is checked, not
  assumed), and otherwise abstains. Vue and Svelte take the same path — there is
  no per-framework branch — and a JavaScript-authored carrier (published as
  `.jsx` for the editor) still presents a TypeScript surface.
- **WHEN it is servable** — the host. For `@verter/typescript-plugin`
  (`index.ts::importedCarrierForSource`): published ⇒ resolve; owned but not yet
  published ⇒ the bounded cold read (`helpers/coldRead.ts` — last-good, else a
  short bounded block on the manifest), the same wait `readFile`/`fileExists`
  perform for a known companion, so resolution and serving cannot disagree; not
  owned, or the cold read times out ⇒ ABSTAIN.

**ONE target for every host that can choose, and there is no per-host mode.**
Both hosts that serve carriers through a `resolveModuleNameLiterals` override —
the tsserver plugin and the browser/WASM in-context service
(`packages/playground/src/editor/inContextLs.ts`) — call the same function and
get `Comp.vue.verter.ts`. A host that rewrites the specifier itself can name any
published surface, so "which one" is a POLICY question with one answer, not a
capability question with several.

This matters semantically, not just cosmetically: the two published surfaces are
NOT interchangeable. A runtime-object `defineExpose({ count })` renders
`count: typeof count` on the API surface (the setup body is emitted, so the
inferred type survives) and `count: unknown` on the declaration surface (the body
is omitted, so the binding is out of scope — see the TODO in
`crates/verter_compiler/src/tsc/script.rs`). Letting a host pick the declaration
carrier would make the same source type differently depending on where the user
reads it. Pinned at the type level by
`cross_host_import_surface_semantic_parity (#11)` in
`packages/playground/src/editor/wasmInContextLs.spec.ts`.

The engine that gets no say is **native tsgo**: it has no host plugin, so
TypeScript's own basename-append probe finds the `importDriven` declaration
carrier (`Comp.d.vue.ts`) and nothing else
(`crates/verter_type_runtime/tests/cases/owned_provider_carrier_resolution.rs`).
That path never reaches this policy, so it must not shape it — and it is a
strictly weaker surface, which is why closing the `script.rs` gap is a
precondition for relying on it. `carrierRootMembership` names the same three-way
split from the program-membership side.

Abstaining is the fail-closed state, not a dead end. Resolving to an unowned path
would produce a sticky `TS2307` no publication could clear; abstaining lets
TypeScript's own answer stand and the next publication heal it. The carrier source
is recorded in the plugin's `observedCarrierImportKeys` BEFORE the resolution
attempt, so a publication that makes it ready counts as a relevant ready-version
change and triggers that configured project's resolution-cache clear
(`clearSemanticCache`); a store-dir handoff (the cold-start window before the LSP
reports the store) clears it too. On the LSP side, `did_open` of a plain script
prewarms each imported carrier's API through
`sync_imported_carrier_api_lightweight` → `publish_carrier_to_external_ts`, and
every publish advertises the COMPLETE companion set.

Alias/`baseUrl` specifiers take the same target through the failed-lookup route;
`baseUrl` containment is boundary-anchored over the host's canonical path identity
(`canonicalPath`), never a raw substring test — a `D:\ws\src` baseUrl must accept
the `d:/ws/src/...` candidates TypeScript reports.

**Import-path completion consumes the same policy.** TypeScript's own
module-specifier path completion never consults `extraFileExtensions`
(`getSupportedExtensionsForModuleResolution` lists TS/JS extensions plus ambient
`*.x` wildcard modules only), so a plain `.ts` buffer typing `from './` was never
offered `./Comp.vue` / `./Comp.svelte`. The plugin augments module-specifier
string positions in `getCompletionsAtPosition` (`index.ts::withCarrierPathEntries`
+ `helpers/pathCompletion.ts`): candidates come from ONE manifest snapshot per
request (`DiskCarrierStoreReader::importCompletionSnapshot` — no directory
listing, no filesystem walk, exactly one manifest stat on the keystroke path
regardless of candidate count) and each must pass `resolveCarrierImportTarget`
PLUS the non-blocking readiness arms of actual resolution (a published
`CarrierApi` import surface in the same snapshot, or retained last-good
content). The guarantee is one-directional and snapshot-relative: every OFFERED
entry's accepted bare specifier resolves against that snapshot; a conflicted
(manifest-absent) carrier, an IDE-role-only carrier, a rune module, and an
owned carrier still in its publication warm-up window (no ready entry, no
last-good — the bounded cold read could still resolve it, but completion never
blocks per candidate) are never offered, and the offered name is never a
companion path. Replacement spans are computed from the literal's RAW source
characters (`rawModuleSpecifierText`), never the cooked `literal.text` whose
collapsed escapes (`.\\W.sv` → `.\W.sv`) desync spans from the file; an escape
form outside the modeled separator escapes fails closed (no entries). Entries
mirror TypeScript's path-entry shape (kind `script`, extension
`kindModifiers`, `SortText.LocationPriority`). The e2e proof on the
VS-Code-TS-service lane is `ide.complete.import-path-carrier` (shared parity
suite, Vue AND Svelte).

**KNOWN GAP — auto-import does not offer unopened carriers.** `getExternalFiles`
advertises only the companions of editor-ACTIVE carrier sources (the authored
working set, matching Volar/Svelte ownership), so an unopened carrier's API
surface is in neither the Program nor a package export map and TypeScript cannot
suggest it. The eager `CarrierApi` index that would close this
(`external_ts_sync.rs::EagerApiIndexPlan`) is landed but has NO production caller —
it is referenced only by its own tests. This is a separate missing contributor
from the redirect-target rule above; an explicitly written import resolves
correctly either way.

### Barrel-Import Eager Sync (TSGO)

When a carrier script imports through a non-carrier barrel (for example `components/index.ts`), publication follows exact configured ownership and each effective owner's `allowImportingTsExtensions` option. For tsserver, every effective owner must explicitly opt in before authored `.vue`/`.svelte` specifiers stay under the plugin + TypeScript resolver with no rewritten barrel buffer; one missing/`false` co-owner keeps the single shared buffer on the `.verter.ts` compatibility projection. Deterministic nearest-config convenience lookup is never ownership authority. TSGO retains explicit publication because it has no equivalent host plugin. Both explicit-publication paths seed from shallow import facts, walk only `ExportFrom` references, sync terminal carrier dependencies first, and publish only barrel projections whose bytes differ from disk; unchanged closure files are never pushed. The walk is cycle-terminated and complete (no depth/node cap may mint a false `DependencyReady` receipt) and yields periodically instead of truncating. Ordinary imports, dynamic imports, `require` edges, and unchanged compiled output remain provider-resolved from disk.

**Nonblocking dependency readiness**: definition and type-definition use capture-only readiness. A missing receipt enqueues/coalesces background publication but never joins it; the request queries the provider immediately against whatever project state is already valid. Rename first performs the latest-current-file interactive repair, then captures readiness without joining. The provider query may still serve symbols whose configured-project graph is already complete, while every confirmed cross-carrier child-prop edit must independently prove both its usage and declaration legs; missing API publication therefore fails that completeness gate closed instead of returning a partial edit. Production navigation never waits for the barrel/publication walk. Settled publication joins exist only as explicit test setup.

**Demand-driven workspace-symbol completeness**: ordinary hover/completion/definition keep only editor-active/import-observed carriers in the provider. References and rename are different: neither tsserver nor tsgo can prove a workspace-wide result unless every framework source in the initiating carrier's configured project is represented. On the first such request, Verter reads immutable configured-project membership. Tsserver proves the frontier by promoting every current store advertisement through one `activate_carrier_members` transaction plus one interactive plugin refresh. TSGO never treats that editor-tsserver store as its witness, and proves TWO independent things about its own provider graph. (1) **Roots**: every expected carrier has receipt-gated, owner-matching IDE direct-open state. The API companion is deliberately not demanded here — the interactive IDE path never opens it for the file under the cursor, so demanding it would gate the frontier on a companion neither arm activates. (2) **Import closure**: for every carrier-import edge reachable from those carriers — direct imports, dynamic module references, and the `export … from` graph through rewritten barrels — the target's API companion (or the barrel's rewritten shadow buffer) is both LIVE and CURRENT. Roots alone prove each carrier's own symbols are in the Program, not that a cross-carrier reference RESOLVES: an importer's buffer imports the rewritten `{carrier}.verter.ts` specifier, so an unopened — or stale — API companion silently drops that importer from a project-wide answer. Liveness is the committed loaded flag; currency is a ROLE-SPECIFIC witness (the identity of the API declarations actually delivered to this provider, which only an API delivery may write — the state-wide commit stamp advances with IDE-only receipts and cannot stand in for it). An interactive IDE sync carries the liveness flag forward (that buffer really is still open) and re-observes the current projection, so an API-NEUTRAL edit stays ready with no reopen while a public-surface edit fails closed until publication delivers the new declarations. **Every writer of a shadow buffer must record the delivery it performed** — the source identity it built from AND the owner the live resolver reports — because the barrel re-publication leg skips an already-live shadow and is therefore not a recovery path: a state that sets only the liveness flag makes a delivered barrel read as undelivered-and-unowned, and the closure then refuses every project-wide references/rename answer for the rest of the session. This includes the OPEN-document own-path sync (`sync_self_file_shadow_state`), which serves the same rewritten projection when a user simply opens the barrel `.ts` in the editor; it recomputes the binding from the live resolver rather than clearing it, so `Unresolved` means ownership is genuinely unpublished. Carriers that are not import targets — a standalone initiating carrier, the file under the cursor — are never gated, and neither are targets outside the configured project. If the frontier is incomplete, the request signals scanner priority and fails closed instead of joining compilation or returning partial references/edits. The activated frontier stays warm for later workspace-symbol requests.

**Optional native semantic enrichment**: production LSP projection hosts use the bounded `AnalysisScope::BUILD` path because IDE projection codegen requires script bindings/macros/exports and lightweight style metadata in addition to shallow import ingress. Full template/style/cross-file Verter semantic/type-info is disabled by default and enabled only by `analysis.enabled` (or an explicitly enabled native lint surface). Native Verter hover semantics are independently disabled by default and require `verter.hover.nativeSemantics`; with that setting off, hover remains provider/CSS owned and does not synchronously enter native semantic queries. When enrichment is enabled, a lazy isolated host with one CPU worker processes immutable document snapshots after a debounce on a serialized background lane; component-meta/public-prop hydration happens on that isolated worker, and results publish only if source and version are still current. Semantic prop facts merge by authored prop name: existing source spans and native-only rows are retained, while semantic-only rows append deterministically. Boolean classification consumes typed `TypeExpr`, never display-text sniffing. Completion consumes only already-published semantic or projection-host snapshots: it never calls `ensure_loaded`, constructs component meta, or enumerates workspace components on the default path. LSP handlers never synchronously reconstruct full enrichment from the projection host, and TypeScript diagnostics/navigation do not depend on enrichment completion.

### Freeze Prevention (Fast Typing)

Three layers prevent tokio runtime starvation during rapid typing:

1. **Latest-value SyncCoordinator** (`sync_coordinator.rs`): `didChange` performs no provider I/O. It commits the registry/host snapshot, replaces the file's pending signal in a map, and best-effort wakes a capacity-one channel. The single coordinator debounces the latest value per canonical file; edits cancel only detached stale diagnostic tasks, never a provider-state commit that may be mid-transaction.
2. **Version-fenced staged push diagnostics**: LSP uses push diagnostics exclusively (no pull/`diagnostic_provider`). After the quiet window, the coordinator computes the provider-free Verter/ownership batch and publishes it immediately if URI, source identity, and exact document version still match. Provider diagnostics run afterward on a cancellable detached task and replace that batch with the merged result under the same exact-version/source fence. A slow or hung provider therefore cannot starve Verter-owned errors/hints, while a newer edit cannot publish an older batch. Completion of optional native semantic enrichment broadcasts a versioned `SemanticReady` event that invalidates only the Verter diagnostic cache and schedules a diagnostics-only pass: it never repeats provider file sync or graph refresh. Broadcast lag recovers by scheduling every open document once; channel closure terminates the coordinator instead of spinning.
   tsserver diagnostics come from the three synchronous pull commands, issued as one ordered background transaction under one editor-idle admission; category-local failures degrade independently while interactive preemption restarts the idempotent transaction. Its pushed `semanticDiag` / `syntaxDiag` / `suggestionDiag` event bodies carry a file but no ScriptInfo version in supported TypeScript protocol versions, so those events are progress/health signals only and never enter the result cache. A last-good synchronous pull is reusable after a transient transport failure only while the provider's globally unique local content generation still matches; edits and close/reopen cycles invalidate it. The LSP's exact authored document-version plus immutable source-identity fence remains the final publication authority, including same-version edits and close/reopen ABA.
3. **Lifecycle health, not feature deadlines** (`verter_type_runtime/src/tsgo/ipc.rs` AND `verter_type_runtime/src/tsserver/ipc.rs`): production feature requests have no wall-clock latency timeout. Dropping a client-cancelled future removes its pending registration and sends engine cancellation. A separate watchdog observes pending work plus complete engine-output silence; sustained silence signals `crash_notify` so the resilient wrapper restarts the provider without returning a fabricated empty result. Engine EOF atomically closes the pending registry, drains in-flight requests, and rejects requests racing after death. Explicit timed helpers remain test/diagnostic-only; initialization, shutdown, writer-stall, and lifecycle bounds are not feature latency budgets.

**Client-process containment**: `verter-lsp` installs process-tree containment before any provider spawn, then binds the standard LSP `InitializeParams.processId` as the authoritative editor-neutral client witness. `--client-pid=<pid>` is only an optional early bootstrap witness and is atomically superseded by the protocol value; malformed or out-of-range PIDs fail closed, and OS parent inference is never used. Windows assigns the LSP to a `KILL_ON_JOB_CLOSE` Job Object before semantic children exist and monitors a stable process handle. Linux monitors a `pidfd`; macOS registers `EVFILT_PROC`/`NOTE_EXIT` with `kqueue`, avoiding PID-reuse races. Every owned tsserver/tsgo tree is armed and registered immediately after spawn. On client death the monitor kills all registered Unix engine process groups before terminating the LSP; Windows process exit closes the outer Job Object and kills the complete subtree. Linux engine children also retain `PR_SET_PDEATHSIG` with the post-arm parent check. Normal shutdown still uses provider `shutdown` plus tree-aware `Drop`; stdio EOF remains the fallback when an LSP client sends no `processId`.

**Interactive-repair singleflight** (`sync_orchestration.rs`): `ensure_current_file_synced` holds a per-document `tokio::Mutex` around the foreground hover/completion/definition repair, with a freshness token re-checked under the lock. A stale open self-file projection (`.svelte.ts`/`.svelte.js` included) is repaired immediately through the same unresolved-shadow primitive instead of waiting for the 300ms coordinator tick. A hover storm coalesces into ONE repair per document per generation (not N concurrent repairs stampeding the provider); `force_reopen_current_file_in_type_provider` takes the same lock. Repair lanes are objects bound to an open-document generation: close removes only its exact generation and retires its exact lane, while the final lease drop removes that lane by pointer identity (event-driven, never polling). Every repair revalidates `(URI, canonical ID, open generation)` after locking, so work paused before lease acquisition cannot recreate a lane after close or retire a reopened document in a canonical-key ABA race; a repair that acquired its lease before the close can hold the very lane object a reopen revives in place, so the stale-path retire is gated on the lane still carrying the repair's own generation (the lane generation is reassigned only under the lane mutex, making the check exact). **Crash/recovery hardening**: a failed provider respawn is retried inside the same restart budget (never silently ends the crash monitor); a failed `LazyManagedTypeProvider` activation is retried after a 250ms cooldown (not latched for the session); a tsserver cold-miss `reloadProjects` recovery is singleflighted + rate-limited to one per 2s (a full all-projects rebuild's duration) so concurrent cold queries can't storm rebuilds.

**Completion D1 snapshot invariant** (`lifecycle.rs`, `nav_features.rs`, `features/cursor_context.rs`): `did_change` has two ordered lanes: a short completion-visible commit fence covering only registry/host upsert, plus provider-publication turns enqueued in commit order and awaited only AFTER the commit fence is released. `did_open` / `did_close` registry membership mutations (including virtual open and early close) take that same short fence, but no provider or lifecycle await may occur while it is held. Thus slow provider I/O preserves publication order without blocking later same-document or unrelated registry commits, and membership ABA cannot race a final native calculation. Completion holds the commit fence only long enough to capture one immutable parent `(source, line index, analysis, blocks, canonical ID)` snapshot, then releases it before provider awaits. After any provider await, completion validates both LSP version and immutable source identity (close/reopen may reuse a version); identity advance triggers bounded recomputation. The final native attempt keeps the commit fence through its synchronous cache-only native calculation, so sustained churn returns one coherent current result without panic or stale items. Cold imported carriers belong to the background scanner and TypeScript provider; interactive completion never `ensure_loaded`s them. Root-template ownership is framework-explicit: Vue markup is owned only by an explicit `<template>` block (script-only Vue never borrows Svelte root rules), while Svelte owns carrier-root markup, treats every paired root element/component (including `<template>`) as ordinary markup, and recognizes only `<script>` / `<style>` as SFC blocks. Vue root scaffold snippets never leak at Svelte root whitespace; fail closed until a Svelte-native root producer exists. Optional Svelte public-prop enrichment is built asynchronously in the isolated semantic host and published as an immutable cache snapshot; completion only reads it, preserving authored public keys (aliases, string keys, rest-covered members, named interfaces, and whole-object `$props()`) without parsing source on the request path. The source-authoritative Svelte cursor lexer searches bounded structural tag candidates and tracks nested braces, strings, comments, and regex literals, so `<` inside JavaScript never replaces the owning tag anchor. Component prop label/snippet syntax is owned by the parent carrier language, never inferred from the raw import suffix: extensionless and barrel-resolved Svelte children still use authored keys and `prop={$1}`. Vue attribute syntax is never a fallback for unresolved or zero-public-prop Svelte components; fail closed until a Svelte-native producer owns that surface. Virtual editor projections do not participate in carrier repair generations or lanes, preventing encoded/raw virtual URI identities from splitting lifecycle cleanup.

Provider diagnostics are published only when their generated range maps back to authored source; diagnostics wholly inside synthetic carrier scaffolding remain dropped. If multiple provider reporting sites map to the same current-file identity (range, severity, code, source, and message), the merge publishes one diagnostic and unions tags, related information, code descriptions, and data. Severity is part of identity, so an error and a hint never collapse. Carrier codegen must map the authored reporting token rather than re-anchoring synthetic diagnostics in the merge layer.

### Heartbeat Watchdog

The server sends `$/verter/heartbeat` every 5s from `initialized()`. The VS Code extension monitors heartbeats -- if none arrive for 30s, it auto-restarts the server. Last-resort safety net for runtime starvation.

### Async Workspace Scanning

During `initialized()`, the LSP spawns a `WorkspaceScanner` background task instead of scanning synchronously. The scanner walks the filesystem, compiles registered framework carriers (`.vue`, `.svelte`, and future registry entries), and publishes them through the engine-specific route in priority order:

1. **Tier 0**: Files opened in the editor (signaled by `did_open`)
2. **Tier 1**: Project source files covered by `tsconfig.json` -- siblings of open files first, then expanding outward
3. **Tier 2**: Remaining carrier files not covered by any tsconfig

The scanner receives priority signals from `did_open` to dynamically re-order its queue. Each carrier unit waits for interactive-handler idleness (with a fairness cap under continuous traffic). TSGO wire sync is additionally throttled; tsserver performs one store-refresh notification after the carrier batch and never receives generated-file protocol opens. This keeps `initialized()` independent of the full scan.

**Configured-project discovery**: `verter_workspace::config::discover_tsconfigs` discovers `tsconfig.json`, `tsconfig.*.json`, AND the JavaScript project config `jsconfig.json` (the configured-project authority for JS-only trees that tsserver/tsgo honor natively). A `jsconfig.json` next to a same-directory `tsconfig.json` is suppressed (TypeScript precedence). `has_configured_ts_project_anywhere` and `is_project_config` treat it identically, and `is_config_file` rebuilds the registry on jsconfig edits. Without the jsconfig arm, carriers under a jsconfig-only directory resolve `NoProject` and the tsgo carrier admission gate fails every feature closed (the js-lax D7 defect family).

**Key module**: `crates/verter_lsp/src/workspace_scanner.rs` -- `WorkspaceScannerHandle`, `spawn_workspace_scanner()`, priority sorting, throttled sync loop.

## Ownership Lifecycle & Bootstrap Sync

The VFS publishes workspace snapshots atomically via `PublishedRoot`. Each snapshot carries an `ownership_ready: bool` flag:

- **Bootstrap** (`ownership_ready: false`): `Engine::new()` eagerly publishes an empty snapshot so basic relative resolution works immediately. Ownership queries return no results. Provider path transforms (`provider_id_for_source`, `provider_ide_id_for_source`) are pure -- they work without ownership.
- **Ready** (`ownership_ready: true`): After `background_init` builds the full project graph, a real snapshot is published. Ownership queries are now authoritative.

**Provider sync state uses typed ownership** (`ProviderOwnerBinding`):

- `Provisional` -- file synced before ownership is known (bootstrap).
- `Owned(String)` -- file bound to a real project (tsconfig path or root).

**Readiness-gated sync rules**:

- `ensure_current_file_synced()`: During bootstrap, provisional sync allowed. With a ready snapshot, only files with a project owner are synced -- unowned files are queued in `pending_snapshot_provider_sync` for later drain.
- `sync_imported_vue_api_lightweight()`: Same rule -- provisional sync only during bootstrap.
- `SyncCoordinator::sync_file()`: Always queues files with no owner for retry. Uses `ownership_ready` for log level (warn vs info).

**Key files**:

| File | Purpose |
| --- | --- |
| `crates/verter_workspace/src/published_state.rs` | `PublishedRoot`, `ownership_ready` |
| `crates/verter_lsp/src/provider_sync.rs` | `ProviderOwnerBinding`, `ProviderSyncState` |
| `crates/verter_lsp/src/server/sync_orchestration.rs` | `PublishedResolverSnapshot`, `ensure_current_file_synced` |

### Carrier Owner Selection (tsgo-faithful, single-winner)

A carrier claimed by MULTIPLE configured projects is NEVER a terminal state. `WorkspaceSnapshot::default_configured_owner_for_file` (`crates/verter_workspace/src/workspace_snapshot.rs`) models tsgo `ProjectCollection.GetDefaultProject` + `findDefaultConfiguredProject` (`microsoft/typescript-go` `internal/project/projectcollection.go`):

1. **Claimants** = configured projects that directly include the file (ordered `projects` Vec, never a set). Zero ⇒ `None` (⇒ `NoProject`); exactly one ⇒ that owner; ≥2 ⇒ the walk.
2. **BFS entry** = the nearest ancestor solution — the nearest configured project whose tsconfig BASENAME is the LITERAL `tsconfig.json`/`jsconfig.json` (`computeConfigFileName`), NOT the nearest project root. `tsconfig.app.json` is never an entry; a solution `files:[]` at the same directory is.
3. **BFS** its `references` in DECLARED array order (ordered visited set); the FIRST project that directly includes the carrier wins (stops the search). A `files:[]` solution never wins directly — it fans out to its references.
4. **Climb** to the next ancestor solution (nearest literal config above) unless `compilerOptions.disableSolutionSearching` (default false).
5. **Fallback** = the lexicographically-least `tsconfig_path` among the claimants (tsgo `firstConfiguredProject`) — a name-least ordering DISTINCT from the reference BFS order, drawn from configured claimants only, never an inferred/fallback project.

The winner flows through the SAME `binding_for` → `BoundProject` witness as the unique-owner arm. Only ordered structures decide (no `HashSet` iteration); reference cycles resolve via the visited set. The `external_ts::resolver` REWRITES only its `Ambiguous(ids)` arm to call this selection (the carrier-path-conflict pass stays first and unconditional). This is provider-neutral — the ONE decision tsserver, managed-tsgo, and shared-tsgo all consume.

**Bounded divergence:** `ConfiguredMembership` is include/`files` only and has NO `IsSourceFromProjectReference` data, so every carrier hit is treated as DIRECT (`multipleDirectInclusions` effectively always true). The solution-graph pruning in `configured_owner_resolution_for_file` (leaf-over-referencing-ancestor) is preserved on the `Unique` arm and not unified with the BFS.

**Remaining terminal no-serve states:** ONLY `NoProject` (no configured project's include/`files` covers the extension) and the disk-layout carrier-path conflicts (`carrier_never_shadows_real_user_file` / `same_stem_svelte_component_rune_fails_closed`). `NotReady` is the transient bootstrap retry. Terminal `NoProject` / carrier-path conflicts emit a `verter(project)` warning on `did_open` AND `did_change`.

### One Verter-Owned Diagnostic Set

`server_utils::verter_owned_diagnostics` is the ONE producer of the Verter half of a document's diagnostics, and every publisher calls it: the debounced coordinator (`sync_coordinator::compute_verter_diagnostics` — the `did_open`/`did_change` path), BOTH background-initialization sweeps (`background_init.rs` — post-scan and step 7a), and the pull `textDocument/diagnostic` path (`server::compute_full_diagnostics`). Each publisher REPLACES the client's whole list for the document, so a publisher assembling a narrower set silently erases whatever the others surfaced — last writer wins, with no error anywhere. The version-cached document half (`compute_verter_diagnostics_for_with_views`) is PRIVATE to `server_utils` so a future publisher cannot reach it without the state-derived categories; tests go through the `#[cfg(test)] document_diagnostics_for_test` wrapper.

The document half is cached per `(version, diagnostics generation)`; the categories below derive from workspace/ownership/provider state that moves independently of the document version, so they are recomputed on every publish and never enter that cache.

| Code | Source | Meaning |
| --- | --- | --- |
| `verter(project)` | `project_ownership_diagnostics_for` (`external_ts/carrier_sync.rs`) | terminal `NoProject` / carrier-path conflict |
| `svelte-package-missing` | `svelte_assets::svelte_package_diagnostic` | no `svelte` install governs this document |
| `svelte-package-unusable` | same | a `svelte` install EXISTS but fails validation (wrong name, unsupported major, missing/broken `.` or `./elements` type exports); message names the install and the validator's reason |
| `carrier-provider-unavailable` | `ProjectSync::carrier_preparation_failure` | the carrier's provider surface could not be prepared at all; `carrier_provider_surface` fails closed with `None` and this explains why every provider-backed feature on the file is dark |

**One Svelte owner resolution — WITHIN RUST.** `svelte_assets::resolve_svelte_owner` is the only "is this a usable `svelte` package" answer in the Rust tree, and `SvelteOwnerAnchor` — private, exactly two constructors — is the only way to say where to look. `SvelteOwnerAnchor::document` (the document's own directory, Node's nearest-first rule) anchors BOTH the carrier specialization and the diagnostic, so for any one document the install the Rust carrier acted on and the install the user is told about are the same install. `SvelteOwnerAnchor::project` anchors ONLY the project-global `paths` rows, which TypeScript cannot express per-directory; it makes no per-document claim and never reaches the user. An unusable install is fail-closed like a missing one (`Ok(None)` from carrier preparation, no `svelte` rows) — never an `Err`, which would abort the whole carrier preparation and take every `.svelte` file's provider surface down silently.

**KNOWN GAP — the tsserver plugin is a second Svelte authority.** The guarantee above stops at the Rust boundary. On the editor-tsserver route the carrier reaches tsserver through the publish store (`sync_coordinator.rs`), and `@verter/typescript-plugin` resolves the Svelte JSX shim's own `svelte` / `svelte/*` imports itself, from a `ownerResolutionAnchor` built at the CONFIGURED-PROJECT ROOT (`packages/typescript-plugin/src/index.ts:896`, used at `:1276`ff; present in the loaded `dist/index.js`). That is a project-root anchor, not a document anchor, and Rust does not drive it.

Observable consequence: for a document with a HEALTHY NESTED install under a BROKEN ROOT install, Rust's document anchor resolves the healthy nested one and publishes NO diagnostic, while the plugin binds the broken root — so editor-tsserver serves broken `svelte` types with no Verter explanation. The reverse layout (broken nested, healthy root) disagrees the other way: Rust reports `svelte-package-unusable` while the plugin resolves the healthy root. The Rust tests cover `prepare_managed_tsgo_svelte_carrier` only and structurally cannot observe either case.

Tracked as follow-on work: Rust publishes owner-bound carrier bytes for ALL provider topologies, carrier preparation's `Ok(None)` becomes a closed `NotSvelte`/`Ready`/`Disabled` outcome, and every plugin-side Svelte resolver is deleted.

`ProjectSync` records a carrier-preparation failure under the carrier SOURCE, not the companion path: a first-open failure never commits a provider path, so a companion-keyed record would be unreachable exactly when it matters. It clears on a later successful preparation, on delivery, on a delivered-ledger hit, and on close.

**Rename fail-closed for a resolved multi-claimant carrier:** hover / definition / completion / references / diagnostics serve from the single resolved owner, but a provider rename covers only that one project. A symbol that escapes the owner (exported + imported by a sibling configured project) would rename partially; escape detection needs the cross-project fan-out (future block). `handle_rename` / `handle_prepare_rename` therefore FAIL CLOSED (a clear `verter:` error, no `WorkspaceEdit`) through the shared `rename_request_admission` gate (`server/rename_plan.rs`) over `carrier_is_multi_claimant` (`server/provider_state.rs`) for a resolved multi-claimant carrier — never a silent partial cross-project rename. A uniquely-owned carrier renames normally.

### One Rename-Plan Owner (prepare + rename)

`server/rename_plan.rs` is the single owner of rename ADMISSION and rename CLASSIFICATION, and BOTH `handle_prepare_rename` (`server/rename_prepare.rs`) and `handle_rename` (`server/nav_features_navigation.rs`) consume it — so the prepare handshake cannot advertise a rename the rename handler would refuse.

- **Admission** (`rename_request_admission` ⇒ `Serve` / `Decline` / `Refuse`): an editor-owned carrier rename and a GENERATED virtual buffer answer nothing (the editor keeps its own behaviour); a resolved multi-claimant carrier refuses with the user-visible error above. Identical for both handlers.
- **Classification** (`RenameTargetResolution::resolve`): ONE `classify_rename_target` call over ONE PROVEN-COHERENT document revision, projected into the authored anchor, the same-file completeness proof, the native edit, and the native-CSS exemption — projections of one value, so a mid-flight edit cannot pull them apart. A rename classifies its cursor against three ingredients — `doc.source`, `doc.line_index`, and the analysis — and two independent guards make them describe ONE revision. **The document-commit fence**: `DocumentRegistry::did_change` drops the canonical's validated semantic snapshot and upserts the HOST first, and only after re-compiling the IDE surface writes the new source + line index into `DocumentState`; in between, `get_analysis` has no snapshot to validate and is served by the host, already at the NEW version while `doc.source` still describes the OLD one. Every registry commit (`did_open`, `did_change`, `did_close`) holds `did_change_mutex` across BOTH steps, so taking it here makes the capture atomic with respect to a commit — the same fence the completion path takes for the same triple, released before either caller's provider await. **Fail closed on drift**: the fence orders this capture against registry commits but cannot speak for a host mutation that never took it, so the analysis is used only once the host's source for the canonical is proven byte-identical to the `doc.source` the offsets were measured against — validated AFTER the read, so a host that moved while the analysis was being served is caught too. Any mismatch, or a canonical the host no longer holds, yields `RenameTargetClass::Unavailable`: NO anchor for prepare to advertise and NO range for rename to emit, because version-B spans converted through version-A's line index describe neither revision.
- **Same-file completeness proof** (`SameFileProof`, `RenameTargetResolution::same_file_proof`): what the EMITTED transaction must prove about the REQUESTED file — a projection of the one resolution, and deliberately DISTINCT from the edit set `native_workspace_edit` emits. `Requires { ranges, enumerates_whole_file }` carries TWO separable facts: the ranges the transaction must overwrite, and whether those ranges are the file's COMPLETE authored occurrence set. Only the second licenses delegating a dropped current-companion location to this gate (see the emitted-transaction gates below); a required-but-incomplete set proves the ranges it names and vouches for nothing else. Native/CSS ⇒ requires every same-file occurrence Verter's own typed analysis proved (by construction the set Verter itself emits), and claims the whole file exactly when the classifier's `SameFileEnumeration` witness grants it. A PROVIDER-ONLY instance member ⇒ requires the AUTHORED TOKEN under the cursor and claims NOTHING more: Verter owns no occurrence of its own there (claiming one would rewrite a same-named script declaration, a DIFFERENT symbol) and emits nothing, but a transaction that does not overwrite the requested token renamed something the caller did not ask for — so the anchor is required, and it is the same exact range prepare requires a provider location to map back onto before offering anything. That class with no convertible anchor ⇒ `Unprovable`, which refuses. `Unavailable` ⇒ requires NOTHING and claims nothing: it must not suppress a provider-owned result at a position Verter cannot classify at all, and it must not vouch for one either.
- **The completeness witness** (`SameFileEnumeration`, `features/rename.rs`): whether a claim is the file's WHOLE authored occurrence set. Two conjuncts, each a POSITIVE fact. SCRIPT is always enumerated — every `<script>` block's content is searched exhaustively for the identifier, so no script spelling can be missing. MARKUP is enumerated only when the owner GRANTS it (`RenameTarget::grant_markup_occurrence_enumeration`); the classifier leaves that conjunct ungranted, so the DEFAULT is fail-closed and a caller that never resolves the carrier capability holds a strict-subset claim. A `<style>` `v-bind()` expression naming the identifier is a TERMINAL `Partial` (`style_vbind_roots` names those roots exactly) that no grant can clear. What is deliberately NOT the witness: the mere EXISTENCE of a template snapshot (a Svelte carrier HAS one with `binding_occurrences` / `unresolved_bindings` permanently empty, so `is_some()` would vouch for markup that was never modelled) and a lexical scan of the source (false NEGATIVES on exactly the framework spellings that matter — `$count` and `:my-prop` are different words from `count` and `myProp`, so a scan would report every spelling accounted for while the authored occurrence sits unclaimed).
- **The markup capability** (`markup_occurrence_inventory`, `server/rename_plan.rs`): the one place the grant is decided, resolved from the file's carrier row so the classifier never learns which framework it is looking at. A SVELTE carrier models no markup occurrence, so its claim can never be the whole file and its rename REFUSES a dropped current-companion leg rather than shipping a script-only partial that leaves an authored `$count` bound to the old name — restoring the "fails closed, never wrong" property of the empty-markup-inventory gap instead of voiding it. A non-carrier file claims nothing (its provider buffer is its own, and rename is deferred for a self-file projection, so no companion drop can arise). DURABLE HOME: a capability column on the framework adapter descriptor; until it exists the polarity here follows the carrier-routing architecture guard (reference carrier is the default, a deviating carrier is named), so a NEW carrier that does not model markup occurrences must declare itself.
- **Prepare projection** (`RenamePlan`): `Offer(range)` where Verter's native or CSS analysis is the authority; `ProbeProvider { anchor }` where the TypeScript provider is the SOLE authority, in which case prepare ASKS the provider and offers the anchor only if a returned location maps back exactly onto the authored token (provider absent, self-file rune projection, unmappable cursor, incomplete carrier frontier, error, empty answer, superseded surface, or geometry that misses the anchor all offer nothing); `Decline` otherwise. Because the server advertises `prepareProvider: true`, a `null` prepare means the editor never sends `textDocument/rename` — so declining without asking would silence the only authority that can answer.
- **Per-request, never transferable**: a resolution is never cached across requests. `handle_rename` re-resolves, re-queries, and re-validates its own captured surface; a yes-prepare licenses none of its gates.
- **Emitted-transaction gates**: three fail-closed proofs over the `WorkspaceEdit` the editor receives, each asserting exact ranges and never provider counts. (1) **Unguarded drops** — `merge_rename_locations` returns a `RenameMergeOutcome`: the transaction PLUS every provider location it could not map to a source edit, each as a `DroppedRenameLocation` carrying its own path and a typed `RenameDropReason`. `unguarded_rename_drops` (`server/nav_features_navigation_rename_gate.rs`) refuses the WHOLE rename — no partial, not the same-file half, not the verter-only remainder — when any drop names a file this transaction does not edit: a FOREIGN carrier companion, a carrier PUBLIC-API surface, or a real `.ts`/`.js`. The provider computed those offsets from that file's own content, so the occurrence is real and shipping the remainder leaves that file bound to a name which no longer exists. A drop on the CURRENT request's own companion is delegated to gate (3) — but ONLY when gate (3)'s proof claims the WHOLE file (`SameFileProof::enumerates_whole_file`). The delegation's justification is that an unmapped offset in the generated projection is what SYNTHETIC generated code looks like there: the IDE surface deliberately re-spells authored bindings in unmapped constructs (the setup-return shim emits `doubled: doubled as unknown as typeof doubled`), a real provider reports every one of them on every rename, the mapper cannot tell "synthetic" from "authored but mis-mapped", and refusing on any such drop would refuse essentially every real rename. That justification holds only where gate (3) is a strictly better oracle — where it re-requires every authored occurrence in the file, so an authored occurrence behind the drop resurfaces as a missing REQUIRED range. With a strict-SUBSET proof the delegation covers nothing and the drop is UNGUARDED: a provider-only instance member (whose proof is the cursor token alone, so the file's other spelling of the same member is behind neither gate), an `Unavailable` position (which requires nothing), and a carrier whose markup occurrences are modelled nowhere (every Svelte carrier) each refuse the whole rename instead. Path identity alone never decides it. The signal is per-location provenance rather than an edit count because the merge's `(uri, range.start)` dedup legitimately collapses two locations onto one edit, so a count could not tell a complete transaction from a partial one. (2) **Cross-file child-prop** (`gate_cross_file_child_prop_rename`) — a CONFIRMED `<Child prop=…>` rename must edit BOTH the prop declaration AND the parent `.vue` usage at their exact FULL ranges (start and end) with the new name, or the whole rename returns nothing; provider-agnostic, so a declaration leg from a provider's own native edit satisfies it exactly as Verter's synthesis does. (3) **Same-file** (`same_file_rename_is_complete`) — every range the proof above claims must be overwritten with the new name, the file matched by canonical path rather than URI spelling.

The positional instance-member rule (`offset_is_instance_member_access`, `features/rename.rs`) has exactly ONE definition: `classify_rename_target` reads it for rename, and `features/references` consumes that same predicate for the references half of the identical symbol-identity question.

### Editor-Liveness Provider-Sync Invariant (CRITICAL)

Open-document provider-sync state is an editor-liveness invariant. An OPEN `.vue` document must keep a usable IDE TSX (`{src}.vue.tsx` / `.jsx`) live in the type provider so hover / completion / diagnostics keep working — regardless of ownership. Every `.vue` IDE/API provider sync, in every context (snapshot drain, aliased-import resync, barrel Vue-dependency pass, foreground `ensure_current_file_synced`, debounced `SyncCoordinator`, background API twin, workspace scanner), obeys the same discipline:

- **Ownership None / ambiguous never removes an open `.vue`'s state nor closes its IDE TSX.** A ready snapshot that resolves no owner for an OPEN document converts the committed state to `Unresolved` (preserving the owner-independent IDE TSX) and keeps the file QUEUED for a future owner — it does NOT clear state or close the TSX. Only a genuinely non-open import is removed + closed.
- **Owner transitions sync-new → commit → close-stale, per kind (never close-before-sync).** On an owner change the new paths are opened/updated FIRST; only after a kind's replacement sync succeeds is its genuinely-stale path closed (`genuinely_stale_after_sync` gates on synced-kind AND non-active). A kind whose replacement sync FAILS reverts to its previous live path (`revert_unsynced_kinds`) and that path stays open — a failed reconciliation leaves the prior path alive. On total failure nothing is committed or closed.
- **Owner mismatch / loss forces reconciliation, not only `is_unresolved()`.** A previously-`Owned` open `.vue` whose owner changed or disappeared must be reconciled even when its IDE is already synced (`committed_binding_matches_current` / `current_owner_binding_for_source`); a fully-loaded import is only short-circuited when its committed binding still matches the live resolution. The owned→unowned conversion drops the owner-derived `.vue.ts` API path AND closes it (`dropped_api_path_on_unowned_conversion`) — that path is invalid once unowned — while never closing the IDE TSX.
- **Partial sync stays queued for retry.** A drain pass that syncs one kind but fails another returns `SyncOutcome::Partial`; the drain dequeues only on `FullyReconciled`, so a failed kind is retried rather than permanently suppressed.
- **`active_ide_path_for_uri` is state-backed only.** Interactive routing reads the committed `ProviderSyncState`, never re-derives a path from ownership at request time.

**Guards**: `vue_sync_functions_never_inline_close_the_stale_set` + `guard_detector_discriminates_inline_close_from_delegation` + `vue_sync_functions_never_delegate_raw_stale_close` + `delegated_close_detector_discriminates_vue_evasion_from_approved_and_non_vue` (the static `editor_liveness_guards` architecture guard — source-scans every LSP provider-sync file and FAILS if any function other than the approved leaf close-dispatch primitives contains an inline provider-close loop, OR if any `.vue`-syncing function delegates a raw transition stale set to a close helper before syncing; the delegated-close detector uses fixed-point `let`-taint propagation + recursive close-arg inspection to catch wrapper/`.clone()`/by-value/multi-hop-alias/`Vec`-collect evasions). The per-kind close-after-sync helpers (`revert_unsynced_kinds`, `genuinely_stale_after_sync`, `dropped_api_path_on_unowned_conversion`, `committed_binding_matches_current`) and their integration paths are pinned by the discriminating drain / aliased-resync / barrel / foreground regression tests in `provider_sync.rs` + `server_tests.rs`.

## Multi-Root Workspace & Per-Project Configuration

In monorepo / multi-root VS Code workspaces, different packages have different `tsconfig.json` paths aliases, `.verterrc.json` lint rules, and `vite.config` resolve aliases. The LSP stores all workspace folders (`workspace_roots: Mutex<Vec<String>>`) and builds a `ProjectRegistry` grouping per-project config.

**Key types** (`crates/verter_lsp/src/config.rs`):

- `ProjectConfig` -- per-project: root path, `ResolvedLintConfig`, `Linter` instance, optional `vite_config_path` and `vite_config_deps`
- `ProjectRegistry` -- sorted by root length (longest prefix first), provides `find_project()`, `find_project_root()`, `linter_for()`
- `RegistryBuildResult` -- returned from `from_workspace_roots()`, contains `registry` + `trust_required` list

**Import resolution** (single VFS authority): All LSP import resolution goes through `WorkspaceAccess::resolve_import()` via the VFS `FilesystemWorkspace`. The workspace is created in `initialize()` with an empty project graph (enabling relative/node_modules resolution immediately), then `background_init` populates the full project graph via `set_project_graph()` for alias resolution. The host's internal `project_resolver` (set via `set_internal_resolver()`) is used only for compilation -- never for LSP resolution. `preferred_specifier()` provides reverse-alias lookup for auto-imports.

**Tsconfig/vite config discovery** delegates to `verter_workspace::config` -- all tsconfig parsing, membership, references, and `raw_paths_json` live in VFS. Fallback projects (no tsconfig) get Vite aliases via two-tier analysis in `vite_config.rs`:

1. **Static analysis** (OXC): Parses `vite.config.{ts,js,mjs,cjs,mts,cts}` without executing code. Handles object/array alias forms, `defineConfig()`, template literals, `path.resolve()`, `new URL()`, `fileURLToPath()`. Returns `Complex` for configs using env vars, dynamic imports, or non-allowlisted packages.
2. **Trusted execution** (opt-in): For complex configs, spawns Node.js with `loadConfigFromFile` if the file is in `verter.viteConfig.trustedFiles`. Includes env sanitization, 10s timeout, last-known-good caching.

The server sends `$/verter/viteConfigTrustRequired` notifications for complex configs not yet trusted, and the extension shows a trust prompt. Config file changes (detected via file watcher) trigger a full registry rebuild.

**Type provider integration**: TSGO receives `workspace/didChangeWorkspaceFolders` notifications. tsserver uses per-file `projectRootPath` from the project registry. Both resilient wrappers store workspace folders for restart replay.

**Per-file project-binding authority**: a provider that stamps a per-file project binding takes it from `TypeProvider::set_project_ownership(Arc<dyn ConfiguredOwnerAuthority>)` (`verter_type_runtime::traits`), installed by `background_init` from the published snapshot before any folder sync or file re-open. The authority answers a three-state `ProjectOwnership`: `Owned(ConfiguredOwner { root, config_path })` or the terminal `NoProject`, with the TRANSIENT bootstrap state expressed by the absence of an authority (`Option<Arc<dyn ConfiguredOwnerAuthority>>` on the provider) — three distinct states, never two, because "no authority yet" licenses a last-resort binding and "no configured project claims this file" forbids one. Both halves of `ConfiguredOwner` are load-bearing and neither derives the other: the root is where the project's `node_modules` live, the config is the project's identity and options. One directory routinely holds several configured projects (`tsconfig.app.json` + `tsconfig.node.json`), and a project may be configured by `jsconfig.json` or any `tsconfig.*.json`, so a consumer handed only the root must guess which config applies. The single implementation, `verter_lsp::configured_owner::SnapshotOwnerAuthority`, delegates to `WorkspaceSnapshot::default_configured_owner_for_file` (reverse-mapping a generated companion to its carrier source through `classify_carrier_companion` first) and answers `NoProject` when that names no owner — it never substitutes a nearest-ancestor configured project, whose `include`/`files` by construction do not cover the file. Workspace FOLDERS are never the answer either: one folder can hold many configured projects, and a folder-derived root makes a nested package's own `node_modules/typescript` look absent. `ExtensionTypeProvider::project_binding_for` maps the three states onto `FileProjectBinding::{Configured, Bootstrap, Unowned}`: `Configured` stamps root + config; `Bootstrap` (pre-snapshot only) stamps `longest_project_root` over folders and declares NO config rather than guess one (the consumer then discovers a config itself — `tsconfig.json` then `jsconfig.json`, parsed against THAT config's own directory, failing closed if it exists but cannot be read); `Unowned` fails closed with a typed provider error and declares nothing.

**Rebinding after the authority lands**: `background_init` installs the authority and then calls `resync_open_files`, which providers that stamp a per-file binding MUST implement — `ExtensionTypeProvider` closes and re-declares every live file against the current authority (dropping a file that is now `Unowned`). Nothing else can repair a bootstrap binding: an ordinary `update_file` on an open file sends `changedFiles`, which carries no root and no config, so no number of edits can move a file between projects.

**Lock ordering** (prevents deadlocks): `workspace_roots` (async) -> `project_registry` (sync read) -> release -> `fallback_linter` (sync read). Never acquire `fallback_linter` while holding `project_registry`.

## Async File Scheduler (`verter_scheduler`)

Per-file async staging with priority queuing. Files progress independently through **Source -> Analysis -> Artifact** stages. Cross-file blocking (macro type deps, external `src`) is declarative -- the scheduler manages wakeups.

**Key types** (`crates/verter_scheduler/src/`):

- `FileNode` (`node.rs`) -- per-file: ArcSwap snapshots, AtomicU64 generation, pending requests
- `Scheduler` (`scheduler.rs`) -- DashMap of FileNodes, JobIndex, SubmissionInbox, driver thread
- `CompletionHandle<T>` (`job.rs`) -- request-scoped, resolves to Ready/Failed/Superseded/Shutdown
- `StageExecutor` (`executor.rs`) -- trait for plugging host parse/analysis/compile logic
- `Priority` (`stage.rs`) -- 4 tiers: Critical > Interactive > Background > Maintenance
- `EdgeManager` (`edges.rs`) -- ReverseIndex + BlockerRegistry (both DashMap-sharded)
- `OverlayMap` (`overlay.rs`) -- concurrent editor buffer storage (DashMap)
- `SourceLoader` (`source_loader.rs`) -- MemorySourceLoader (tests/WASM) + DiskSourceLoader (native)
- `IoPool` (`pool.rs`) -- bounded I/O thread pool, separate from rayon CPU pool

### Snapshot Model

All stage outputs are immutable `Arc`-wrapped. Replacement is atomic via ArcSwap. Generation fencing ensures coherence: `source.gen == analysis.gen == artifact.gen == node.gen`. Stale snapshots (gen mismatch) return `None` from `current_*()` methods.

### Host Integration

Feature-gated (`scheduler`): `VerterHost` holds an `Arc<Scheduler>`. During `upsert()`, the host submits to the scheduler, awaits the `CompletionHandle`, reads back the result, populates the compile cache. The `HostStageExecutor` calls real `parse_vue_snapshot`/`parse_non_sfc_snapshot` for the Source stage. Host-specific data is stored in snapshots via the `SnapshotData` trait (opaque `Arc<dyn Any>`), avoiding circular dependencies between scheduler and host.

### Batch Compile & Concurrency Model (current state)

`compile_many(inputs, CompileBatchOptions)` is the host-backed parallel SFC batch compile. Concurrency is **construction-time**, not per-call:

- **Worker count fixed at host construction** via `HostConfig::host_cpu_threads`. The host owns a `verter_scheduler::HostCpuPool` coordinator (shared by every host batch API); never resized per call. `CompileBatchOptions` carries only `priority` + `default_mode` — there is **NO** `threads` / `thread_count` / `num_threads` field (locked by `compile_batch_options_has_no_thread_field` static guard). A per-call concurrency cap (`CpuConcurrencySemaphore`) is a Block-7 design concept, not on the tree.
- **Source upserts route through the one upsert engine.** `compile_many`'s source-updating stage calls `upsert_many_with_priority`, landing ONE `Scheduler::submit_batch_atomic` + ONE `wait_batch` for the whole batch (atomic admission — no per-file upsert fan-out).
- **Per-input requested mode, classifier-owned actual mode.** Each input carries a `requested_mode` (`CompileBatchInput.requested_mode`, defaulting to `CompileBatchOptions.default_mode` → `Session`). The `compile_cache_mode` classifier is SOLE authority for `actual_mode`: `Session` stays `Session` under every eligibility reason (its fact rail handles them); `Content` downgrades to `Stateless` on any reason (its pure key cannot represent cross-file / session-scoped input); `Stateless` is the floor. Compile dedup keyed by `(canonical, effective requested_mode)`.
- **Svelte `cssHash` override — cache identity + fail-closed content admission.** A resolved Svelte `cssHash` override (the callback is resolved OUTSIDE the compiler; only the resolved `Option<String>` threads in) is COMPILE-OUTPUT PROFILE identity, carried on `CompileProfile.svelte_css_hash_override`. It participates automatically in BOTH cache keys — `compile_profile_hash` (the session slot u64) and `content_mode_profile_hash` (the Content pure key). Because the session slot addresses on the u64 alone, `CompileOutputNodeFactValidatedSession::lookup` ALSO re-checks the exact `Option<Arc<str>>` override on the slot against the live value (`slot.css_hash_override != live` misses), so a u64 collision can never serve a result with a different scope hash ("never wrong"). A user-supplied override is not provably content-deterministic, so `classify_compile_mode` pushes `DowngradeReason::CssHashOverridePresent` when one is present ⇒ a requested `Content` compile fail-closes to `Stateless`; `Session` caching stays safe via the profile identity + the exact slot check. The override never overloads Vue's `component_id`; a static guard bans `component_id` reads from Svelte CSS hashing.
- **Session-only compile-tier prefetch.** The cold compute installs `prefetch_compile_tier_observation_targets` (cross-file import-route cache + dependency `IndexedReady` pre-population) ONLY for `actual_mode == Session`, because the compile-tier fact tracer it feeds is installed only for `Session`. `Content` / `Stateless` compile correctness (external `src=` resolution, macro-type collection, dep sync) is produced independently by `compile_entry`.
- **Target-sensitive macro demand.** `compile_entry` requests runtime, TSC, or both from the TypeInfo producer based on `CompileTarget`. Empty output still replaces the semantic transitive-dependency axis with an empty set.
- **Typed macro degradation.** Producer entries carry `Complete`, `Partial`, `Unresolved`, or `Unsupported` outcomes. The compiler accepts only a complete projection with the expected role; missing bundles, degraded entries, or role mismatches fail closed at the authored macro/type anchor. No member is silently reconstructed from parser semantics.

### LSP Integration

LSP file ingestion goes through the one shared upsert engine: `did_open`/`did_change` call `VerterHost::upsert` (→ `upsert_many_with_priority` → one `Scheduler::submit_batch_atomic`), which owns generation tracking, request-context propagation, post-commit cache invalidation, and the canonical-uniqueness contract. No separate LSP-side `submit_request` shim — a file is never source-updated outside the engine (the sole direct `submit_request` is `host_lifecycle.rs` disk-reload with `source: None`, a read). `compile_blockers.rs` is deprecated -- the scheduler's blocker model replaces imperative hydration.

### Authority Chain (Final State)

1. **Scheduler** = sole parser, raw source + analysis authority (`HostSourceData`, `HostAnalysisData`). `HostSourceData::source_type` is the authoritative `oxc_span::SourceType` for downstream cache-key sites -- computed once at `execute_source` time from the framework-neutral parse artifact (`HostSourceData::framework_parse: Option<Arc<FrameworkParseArtifact>>`, the carrier payload every host parse slot stores; Vue's `ParsedSfc` sits behind it, reachable only via the blessed `vue_parse()` accessor). Cache-key callers read via `VerterHost::authoritative_source_type_for(canonical)` or the higher-level `imported_eval_source_type_for(...)` helper.
2. **compile_cache** (`DashMap`) = profile state authority (compile_slots, overrides, diagnostics, deps, resolved_type_hashes). `CompileCacheEntry.evicted_whole_hash: Option<Hash16>` carries the pre-evict hash; `ensure_loaded` compares it to the post-reload hash and skips `bump_store_view_epoch` on no-op reloads so thread-local caches stay warm.
3. **files** (`Shared<FxHashMap>`) = WASM-only primary store. Not used on native (scheduler) path. Gated `#[cfg(target_arch = "wasm32")]` / `#[cfg(any(target_arch = "wasm32", test))]` after the Phase 1 cleanup.

Architectural target for the project-global cache overhaul:

- scheduler remains the sole source and parse authority
- the canonical post-parse artifact above scheduler is `IndexedReady`
- `IndexedReady` owns canonical imports/exports and the owned lowered shallow symbol representation needed by later resolver work
- `AnalysisReady` should build on and enhance `IndexedReady`, not replace it with a parallel file-understanding path
- component-meta and analysis-triggered symbol expansion should populate the same host-owned resolver caches

### Host Store View (Post-Request-View Cutover)

The `CURRENT_REQUEST_VIEW` thread-local, `EffectiveView`, and `*_in_view` helpers are retired; `RequestStoreView` (`crates/verter_session/src/resolver_core/request_store_view.rs`) itself remains a live `pub(crate)` overlay `StoreView`. Resolver-path helpers take `&HostStoreView` (or use the host's live probes directly). `HostStoreView::from_host(self)` snapshots a cheap immutable view of the host's current state; its cache-validation identity is the complete `StoreViewValidationToken`, while `StoreView::compat_token` returns the narrower `StoreViewCompatToken` lane identity (epoch + session + `validity_fingerprint` = the external-supersession fold) — see "Store-View Token, Lane Identity, and Singleflight" below.

Key rules:

- Resolver-path helpers read state directly from the host and `ProjectTypeStore`. No request-private extension store — canonicals loaded mid-request via `ensure_loaded` publish to `ProjectTypeStore` / `FileArtifactStore` and become visible to all readers.
- `VerterHost::is_evalable(canonical)` is the canonical shallow-probe API; it calls `get_whole_hash(canonical).is_some()` directly.
- `ensure_loaded` publishes to the scheduler + `FileArtifactStore`; no extension-store plumbing.
- Cache-validation staleness is enforced by the `ReadSetSignature.facts` path-precise fact signature on cache entries. Warm hits revalidate every recorded fact against the live `StoreView` before returning; stale entries miss and force a cold rebuild.
- Host-scoped caches (final `ComponentMetaResultDb`, `OwnerImportSurfaceDb`, `SemanticGraphStore`) validate through dep-signatures; transient `TypeSurfaceDb` writes only happen through `publish_with_facts` which attaches dep-signatures.

Native canonical loading goes through `ensure_loaded` — the scheduler is the sole source authority. Disk-read fall-throughs were deleted on native. WASM keeps the `files` map + workspace fallback.

### Store-View Token, Lane Identity, and Singleflight

`HostStoreView` is Arc-backed by an immutable `StoreViewSnapshot`: `VerterHost::store_view_manager()` hands out ONE shared snapshot per `StoreViewValidationToken` generation by cheap `Arc` clone; `with_session_overlay` re-roots overlay/tombstone canonicals via copy-on-write so the shared base is never mutated in place. `RequestStoreView` (`resolver_core::request_store_view`) is the LIVE request-scoped read-through wrapper: it chains a `CanonicalCompletionOverlay` in front of the request-entry `HostStoreView` so mid-request additive loads (`ensure_loaded`/`ensure_indexed_ready_serve` successes the entry snapshot did not track) validate without a false miss. The `CanonicalCompletionOverlay` also carries the request-scoped SUCCESS-ONLY session-overlay prepared-decl bundle memo (`overlay_bundle_memo_get`/`_insert`, consulted by `prepared_decl_bundle_with_context` via `ResolverContext::request_completion_overlay`): R17 keeps overlay-bearing bundles OUT of the shared `prepared_decl_bundles` cache, so this per-request memo — keyed `(raw overlay owner, overlay content hash, StoreViewCompatToken)`, admitting only materialisations whose `with_cacheability_scope` verdict stayed clean — is their sole reuse tier; the compat-token key dimension makes a stability-retry attempt under an externally-moved view miss and re-materialise (provenance counter `overlay_bundle_memo_hits`; regressions in `crates/verter_session/src/overlay_bundle_memo_tests.rs`).

**Reuse/validity oracle = the complete `StoreViewValidationToken`:** `store_view_epoch` + `project_generation` + `FileArtifactStore.artifact_generation` + `load_generation` + the workspace `content_generation` + folded env hashes + project identity + frozen overlay identity. `store_view_epoch` is an INPUT to the token, not the oracle by itself. Token-advance rules — the token advances on EVERY state change a base view snapshots by value:

- Every `FileArtifactStore` keyed insert/replace/evict/GC/per-canonical-retention sweep and every augmentation-index mutation bumps `artifact_generation`. `IndexedReady` is the single per-file artifact: route facts come from the current-content `IndexedReady` surface (no separate route-owned artifact), gated by `indexed_surface_is_current` (edge currency + the `project_generation` stamp for surfaces with cross-file edges; a route-resolution mutation drives an edge-refresh materialise that reuses the content-addressed payload and rebuilds the route surface).
- A successful FIRST-TIME additive `ensure_loaded` bumps the dedicated `load_generation` (as does a positive import-route admission via `cache_positive_import_route_result`): it adds a scheduler node + `derived_raw_cache` state the build folds into `whole_hashes`/`derived_hashes` but does NOT publish into `FileArtifactStore`, so `artifact_generation` alone would not cover it. It deliberately does NOT bump `store_view_epoch` — a cold compute's own dependency loads stay EXCLUDED from the publish fence's `externally_superseded_by` check and never self-fence promotion.

**Singleflight LANE identity is NARROWER than the reuse oracle.** The coalescing-lane identity is `StoreViewCompatToken`, whose `validity_fingerprint` is the `lane_fingerprint` — delegating to `external_supersession_fingerprint`, the SAME oracle the promotion fence `is_stable` applies. The fingerprint folds ONLY the external-supersession dimensions: `store_view_epoch` + `project_generation` + workspace `content_generation` (file-set mutations — watcher recovery / dependency appearance — advance it without any host-side epoch; a snapshot's edge-currency gates evaluate against it at build time, so a cached snapshot MUST miss once it moves; a cold compute's own loads never advance it, so it cannot self-fence) + folded env hashes (`env_hash_fold`) + `project_identity` + frozen overlay identity. The additive generations (`artifact_generation` / `load_generation`) are DELIBERATELY EXCLUDED from the lane identity: a cold compute advances them through its OWN work (materialising content-addressed caches gated by `store_view_epoch`, loading its dependencies, admitting its own routes), so two identical concurrent requests snapshot at slightly different points in the load sweep — folding those generations would split them across separate lanes and spawn multiple cold winners instead of one leader + N-1 dedup-joining followers. Because the lane oracle IS the promotion oracle, a follower that joins a lane shares exactly the external dimensions the leader's promotion was gated on, so the leader's dedup-joined result is validation-equivalent for it; and a request whose snapshot externally-supersedes the leader's (an epoch / project / env / identity / overlay change, even at an equal `store_view_epoch`) gets a different lane key and forks its own lane — it never receives a result computed under a different external view. The complete `StoreViewValidationToken` (including the additive generations) REMAINS the store-view reuse/validity oracle — the `StoreViewManager` rebuilds its base snapshot on any additive-generation change; only the LANE identity was narrowed (reuse-oracle = full token; lane-identity = external fingerprint).

**Base-view build:** no-torn-return (a coherent capture or `Superseded`, never a torn publishable view) and SINGLEFLIGHTED — on a token miss exactly one caller sweeps while concurrent token-miss callers wait on a condvar and clone the winner's `Arc<StoreViewSnapshot>` (no N-way parallel sweeps). The component-meta cold publish fence rechecks the computed-under token against the live host before promotion (mismatch → return-only, no shared-cache warm), keying off `externally_superseded_by` so the compute's OWN artifact publications do not self-fence.

**Handle-backed dimensions stay out of the token for DIFFERENT reasons:** `ResolvedImportFactsDb` is content-addressed — its key carries `content_hash`, so a new version is a new key and a fixed handle reads correctly (immutable-by-key). `RouteDb` is NOT content-addressed (`EffectiveExportSetKey` has no content hash; evict/clear/replace reuse the same key) — it stays out because its route-surface validator compares the consumer's recorded `expected_hash` fingerprint against the live `RouteDb` slot, so an evict/replace yields a conservative fail-closed MISS, never a stale positive.

### Non-Current Store-View Contract — Capability Split (CRITICAL)

The general store-view accessor `VerterHost::resolver_store_view()` returns the capability-split `StoreViewRead`, never a raw `HostStoreView`. The raw unwrap that erased the non-current proof is gone: every caller must compile-choose one of two capabilities, so no caller can validate (warm) or return (query) against a known-stale snapshot by accident.

- `StoreViewRead::current()` → `CurrentHostStoreView` — the `StoreViewManager` proved this view current at handoff. Allowed for warm-cache fact validation AND for returning a normal query result.
- `StoreViewRead::into_cold_seed_view()` → `ColdSeedHostStoreView` — usable ONLY for a fenced cold builder. It exposes NO `validates*` surface, so a stale seed cannot reach a fact validator by construction. It carries the read's currentness so a derived context fails closed.

Concrete contract:

- **Warm validators require `&CurrentHostStoreView`.** The top-level warm-validation entry points with no outer publish fence (`try_get_cached_meta_payload`, `ComponentMetaResultDb::get_with_view`, the imported-root / owner-import-surface warm reads) accept ONLY a proven-current view. A `ReturnOnly` read is a cache MISS, not a validation — the caller falls to the cold path whose own `is_stable` / publish fence gates promotion.
- **typeinfo query-returners do bounded-retry-then-supersede.** `resolve_named_symbol`, `evaluate_type_expression`, `project_node_to_type_expr_json_bytes` (the session-owned bytes FFI facade that mints the `OutputProjector` capability + materializes + serializes internally), `resolve_shallow_surface`, `resolve_vue_macro_surface`, `resolve_vue_public_type`, and `resolve_type_with_audit` build a request-bound dispatch context and RETURN the resolved node with NO outer fence. They acquire a `CurrentHostStoreView` via `typeinfo::current_store_view_for_query` (bounded retry, default 3); on sustained churn they surface a typed non-current MISS (`None`, or `QueryError::UnstableState`) rather than resolving against a superseded snapshot and returning a stale node. `None` is the established FFI miss signal (`typeExpr: null`). The retry is bounded — it terminates, never spins. A non-current evaluation must NOT warm the `scratch_cache`.
- **Cold contexts carry `Current` vs `ColdSeed`.** `HostResolverContext::from_current(&CurrentHostStoreView)` vs `HostResolverContext::from_cold_seed(&ColdSeedHostStoreView)` — and the session-bound counterpart `SessionResolverContext::from_cold_seed(&ColdSeedHostStoreView)`. The cold-seed constructor marks the request-bound `RequestStoreView` non-current iff the seed was `ReturnOnly`; its `validates*` family then fails CLOSED, so every nested warm-cache probe inside the dispatch MISSES rather than validating against the stale seed. This is the single-chokepoint enforcement — no individual nested validator knows about currentness.
- **Cold-seed currentness is INTRINSIC to its read — never a separately-sourced flag.** A cold-seed view's `is_current` comes from the SAME read that produced its raw view: `StoreViewRead::into_cold_seed_view()` derives it from the read's arm (`Current` vs `ReturnOnly`). There is no constructor that pairs a raw view with a caller-supplied bool — the retired `ColdSeedHostStoreView::from_raw_for_compute(view, separate_flag)` was exactly such a footgun, letting a view from one read be re-bound with a currentness flag from ANOTHER read (a stale second read marked current). Two valid sources of a cold-seed in a validating cold compute:
  - **A helper that does its OWN fresh read** takes the cold-seed straight from that read: `self.resolver_store_view_read().into_cold_seed_view()` (then `.with_session_overlay(..)` for the session path). The view-bound component-meta cold compute (`VerterHost::view_bound_cold_seed` → `compute_component_meta_state_with_view` / `_from_captured_with_view`) and the bare-host overlay entries (`compute_component_meta_state_with_overlay` / `_from_captured_with_overlay`) use this — currentness and view come from one read, no flag to mismatch.
  - **A helper that holds the EXECUTOR's single-read `(view, is_current)` pair** re-binds it via the SOLE pairing constructor `StoreViewRead::from_executor_snapshot(view, is_current)` (returns the intrinsic-currentness enum, consumed via `.into_cold_seed_view()`), confined to the executor boundary where the pair provably came from one read. The fallthrough cold compute (`compute_fallthrough_surface_uncached`) and the component-meta `*_with_view_arg` entries use this — the executor's `snapshot_view` destructured one `StoreViewRead` and threaded both into `compute`. The executors (`ComponentMetaRequestExecutor`, `FallthroughRequestExecutor`) track `snapshot_view_current` and thread it into `compute(.., base_is_current)`.
- **The cold-seed escape hatch (`ColdSeedHostStoreView::into_inner`) never feeds a validating context (INDIRECT-validation seam).** The raw unwrap `.into_cold_seed_view().into_inner()` DROPS the `is_current` flag. It is confined to NON-validating consumers: the request-driver `snapshot_store_view()` accessors, the overlay-priority `capture_component_meta_inputs_with_view` (builds `CapturedComponentMetaInputs` only), and `#[cfg(test)]` direct-`host` wrappers. The fallthrough resolver validates its per-element / per-child / per-root node-cache entries through the request-bound `ctx.store_view()` (currentness-gated), not a separately-rebuilt raw `HostStoreView`.
- **Fenced cold seeds remain correct.** A real fenced cold builder (`cold_seed_view_and_fence` + `ColdSeedFence`, the request-driver `compute` path) may compute from a `ReturnOnly` coherent seed to avoid blocking, but re-checks currentness before publishing: a non-current seed's result is non-cacheable and surfaces as superseded/degraded, never warmed into the shared cache. Cold builders are NOT forced to `Current` (that would needlessly block cold progress under churn).
- **The raw-view escape hatch is allowlisted.** `StoreViewRead::into_owned_view()` yields a raw `HostStoreView` ONLY for the bare-host owned-view rail (`ResolverContext::resolver_store_view`, reachable when no request-bound context was installed), the request-driver owned-view snapshot accessors (currentness gated by `snapshot_view_is_current`), and `.into_cold_seed_view().into_inner()` fenced cold seeds.

Pinned by the static guards in `crates/verter_session/tests/cases/architecture_guards.rs`: `resolver_store_view_returns_store_view_read` (return type is `StoreViewRead`), `cold_seed_store_view_exposes_no_validation_surface` (no `validates*` on `ColdSeedHostStoreView`), `warm_validation_entry_points_require_current_store_view` (warm validators keep `&CurrentHostStoreView`), `resolver_store_view_into_owned_view_is_allowlisted` (raw-view escape hatch confined to the allowlist), `cold_seed_into_inner_confined_to_non_validating_allowlist` (the `into_cold_seed_view().into_inner()` raw-unwrap that drops currentness is confined to non-validating consumers — the INDIRECT-validation seam), `cold_compute_context_constructors_carry_currentness` (cold-compute context constructors are the currentness-carrying `from_cold_seed` form rooted on `RequestStoreView::new_cold_seed`; the footgun `from_raw_for_compute` stays retired; `from_executor_snapshot` is the sole executor-boundary re-bind), and `cold_seed_currentness_is_intrinsic_to_the_read` (the `(view, flag)` re-bind `from_executor_snapshot` is confined to the executor-boundary allowlist, and a fresh `resolver_store_view_read()` may never feed it — closing the view+flag divergence the constructor-shape guards missed), plus the discriminating regressions in `crates/verter_session/src/store_view_non_current_contract_tests.rs` (`session_cold_seed_context_fails_warm_probes_closed`, `view_bound_cold_seed_currentness_comes_from_its_own_read`, `fallthrough_cold_compute_node_cache_validation_fails_closed_under_churn`).

### Key Files

| File | Purpose |
| --- | --- |
| `crates/verter_scheduler/src/node.rs` | `FileNode` -- per-file state container |
| `crates/verter_scheduler/src/scheduler.rs` | `Scheduler` -- DashMap of FileNodes, driver thread |
| `crates/verter_scheduler/src/job.rs` | `CompletionHandle<T>` -- request-scoped result handle |
| `crates/verter_scheduler/src/executor.rs` | `StageExecutor` trait |
| `crates/verter_scheduler/src/stage.rs` | `Priority` enum |
| `crates/verter_scheduler/src/edges.rs` | `EdgeManager` -- reverse index + blocker registry |
| `crates/verter_session/src/lib.rs` | `VerterHost` -- holds `Arc<Scheduler>`, `compile_cache` |
| `crates/verter_session/src/host_upsert.rs` | Scheduler-driven upsert path |
| `crates/verter_lsp/src/server/mod.rs` (+ 8 siblings) | LSP message loop, request dispatch (Phase 11e split) |
| `crates/verter_lsp/src/sync_coordinator.rs` | Debounced type provider sync |
| `crates/verter_lsp/src/workspace_scanner.rs` | Async priority-based workspace file scanner |
| `crates/verter_lsp/src/provider_sync.rs` | `ProviderOwnerBinding`, `ProviderSyncState` |
| `crates/verter_lsp/src/config.rs` | `ProjectConfig`, `ProjectRegistry`, `RegistryBuildResult` |
| `crates/verter_lsp/src/tsgo/traits.rs` | `TypeProvider` trait definition |
| `crates/verter_lsp/src/tsgo/ipc.rs` | TSGO LSP client, `LspTransport`, hang detection |
| `crates/verter_lsp/src/tsgo/resilient.rs` | `ResilientTypeProvider` (crash detection + auto-restart) |
| `crates/verter_lsp/src/tsgo/project_sync.rs` | `ProjectSync` (batched provider file ops) |
| `crates/verter_lsp/src/tsserver/ipc.rs` | `TsserverTypeProvider`, newline-delimited JSON transport |
| `crates/verter_lsp/src/tsserver/resilient.rs` | `ResilientTsserverProvider` |
| `crates/verter_workspace/src/published_state.rs` | `PublishedRoot`, `ownership_ready` |
