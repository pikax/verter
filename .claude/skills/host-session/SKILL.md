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
| `resilient.rs`    | `ResilientTypeProvider`: crash detection via `Notify`, auto-restart with respawn-retry inside the budget (max 3), file cache replay |
| `project_sync.rs` | `ProjectSync`: batches `open_tsx`/`sync_tsx`/`close_tsx` calls to the provider                 |
| `merge.rs`        | Merges TSGO diagnostics/completions with verter's own results                                  |
| `mock.rs`         | Mock provider for integration tests                                                            |

### tsserver Module (`tsserver/`)

| File           | Purpose                                                                                                                                           |
| -------------- | ------------------------------------------------------------------------------------------------------------------------------------------------- |
| `mod.rs`       | `find_tsserver()` (tsdk -> workspace -> global), `find_node()`, `detect_ts_major_version()`                                                       |
| `ipc.rs`       | `TsserverTypeProvider`: newline-delimited JSON transport, position conversion (byte offset <-> 1-based line/offset), all `TypeProvider` methods    |
| `resilient.rs` | `ResilientTsserverProvider`: same crash/restart pattern as TSGO resilient wrapper                                                                 |

### Provider Selection (`main.rs`)

CLI arg `--type-provider=auto|tsgo|tsserver|off` (from VS Code `verter.typeProvider` setting):

- **auto**: detect TS version in node_modules -- TS 5.x/6.x -> tsserver + recommend TSGO; else try TSGO
- **tsgo/tsserver**: explicit, no fallback
- **off**: verter-only mode

Only one provider runs at a time. Provider PID is sent to the extension via `$/verter/typeProviderStarted` notification for orphan cleanup.

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

**Store-backed carrier refresh**: publishing new carrier bytes must invalidate both tsserver cache classes. `TsserverTypeProvider::notify_carrier_changed` advances the plugin's monotonic `carrierStoreRefreshToken`, which makes the plugin compare manifest identities and reload only changed virtual ScriptInfos, then sends the existing content-preserving `updateOpen` touch for cold negative module-resolution entries. The plugin performs its configured-project reload on the next Node event-loop turn to avoid re-entrant graph mutation inside `configurePlugin`; publication therefore completes one response-bearing `projectInfo` round-trip after those fire-and-forget commands so the next provider query is written on a later host turn, without materialising a project-wide file list or globally reloading projects. An interactive hover that had to repair a stale current-file surface still uses its ordered quickinfo response as the repair-specific synchronization probe and returns only the subsequent refreshed result. Warm hovers remain single-query. Never replace this with an untyped fallback or a global reload on every edit.

**No-silent-empty hover recovery**: a FAILED provider hover (`Err` from transport/engine restart/torn publish) never degrades to a vanishing tooltip — the carrier hover path resyncs the current file and retries exactly once against the freshly captured request surface (provider-neutral, above the per-route trait), validating and merging against the retry surface. A persistent failure fails closed after the bounded retry; a legitimate empty (`Ok(None)`) is not retried.

**Key modules** (`crates/verter_lsp/src/`):

- `tsgo/` -- TSGO integration (LSP client, resilient wrapper, project sync)
- `tsserver/mod.rs` -- `find_tsserver()`, `find_node()`, `detect_ts_major_version()`
- `tsserver/ipc.rs` -- `TsserverTypeProvider`, newline-delimited JSON transport, position conversion
- `tsserver/resilient.rs` -- `ResilientTsserverProvider` (crash detection + auto-restart)
- `workspace_scanner.rs` -- Async background workspace scanner with priority-based file loading

### Background File Sync

During `initialized()`, the LSP spawns a `WorkspaceScanner` background task that compiles ALL workspace `.vue` files to TSX and syncs them to the type provider asynchronously. For TSGO, both `.vue.tsx` (IDE artifact) and `.vue.ts` (public API) are synced; cross-file imports resolve through `.vue.ts` (via `rewrite_vue_imports_for_tsgo`). Ensures imports of non-open `.vue` files resolve to actual component types rather than the wildcard `declare module '*.vue'` fallback.

### Barrel-Import Eager Sync (TSGO)

When a Vue file imports components through a barrel (non-Vue re-export file like `components/index.ts`), the LSP eagerly syncs the barrel and its Vue dependencies to TSGO during `did_open` and `resync_aliased_imports_for_open_files`. Process: (1) discover barrels from `TemplateComponentUsage.import_source` resolving to non-Vue files, (2) scan barrel's `module_references` for `.vue` specifiers, (3) sync Vue dependencies first, (4) sync barrel file. Without this, TSGO only receives barrels from the background scanner, which may not complete before hover/completion requests.

### Freeze Prevention (Fast Typing)

Three layers prevent tokio runtime starvation during rapid typing:

1. **SyncCoordinator** (`sync_coordinator.rs`): Single long-lived task replaces spawn-per-keystroke debounce. Uses mpsc channel + 300ms deadline map to guarantee exactly one sync per file after typing stops. After syncing, computes and publishes merged (Verter lint + TS type) diagnostics via push. Holds shared `Arc<VerterHost>`, `ProjectSync`, `TypeProvider`, `cached_verter_diags`, `PositionEncodingKind`.
2. **Push diagnostics only**: LSP uses push diagnostics exclusively (no pull/`diagnostic_provider`). During typing, no new diagnostics published -- VS Code auto-adjusts existing push diagnostic positions as the document changes. SyncCoordinator publishes fresh merged diagnostics after 300ms of silence.
3. **Hang detection** (`tsgo/ipc.rs` AND `tsserver/ipc.rs`): both transports track `consecutive_failures` (AtomicU32). After 3 consecutive request timeouts, fires `crash_notify` to trigger the resilient wrapper's restart machinery — a wedged-but-alive provider is restarted, not silently timed out for the session. Notifications use `try_send()` (non-blocking) to prevent channel backpressure.

**Interactive-repair singleflight** (`sync_orchestration.rs`): `ensure_current_file_synced` holds a per-document `tokio::Mutex` around the foreground hover/completion/definition repair, with a freshness token re-checked under the lock. A hover storm coalesces into ONE repair per document per generation (not N concurrent repairs stampeding the provider); `force_reopen_current_file_in_type_provider` takes the same lock. Repair lanes are objects bound to an open-document generation: close removes only its exact generation and retires its exact lane, while the final lease drop removes that lane by pointer identity (event-driven, never polling). Every repair revalidates `(URI, canonical ID, open generation)` after locking, so work paused before lease acquisition cannot recreate a lane after close or retire a reopened document in a canonical-key ABA race; a repair that acquired its lease before the close can hold the very lane object a reopen revives in place, so the stale-path retire is gated on the lane still carrying the repair's own generation (the lane generation is reassigned only under the lane mutex, making the check exact). **Crash/recovery hardening**: a failed provider respawn is retried inside the same restart budget (never silently ends the crash monitor); a failed `LazyManagedTypeProvider` activation is retried after a 250ms cooldown (not latched for the session); a tsserver cold-miss `reloadProjects` recovery is singleflighted + rate-limited to one per 2s (a full all-projects rebuild's duration) so concurrent cold queries can't storm rebuilds.

**Completion D1 snapshot invariant** (`lifecycle.rs`, `nav_features.rs`, `features/cursor_context.rs`): `did_change` has two ordered lanes: a short completion-visible commit fence covering only registry/host upsert, plus provider-publication turns enqueued in commit order and awaited only AFTER the commit fence is released. `did_open` / `did_close` registry membership mutations (including virtual open and early close) take that same short fence, but no provider or lifecycle await may occur while it is held. Thus slow provider I/O preserves publication order without blocking later same-document or unrelated registry commits, and membership ABA cannot race a final native calculation. Completion holds the commit fence only long enough to capture one immutable parent `(source, line index, analysis, blocks, canonical ID)` snapshot, then releases it before provider awaits or potentially cold component-meta work. After any provider await, completion validates both LSP version and immutable source identity (close/reopen may reuse a version); identity advance triggers bounded recomputation. The final native attempt keeps the commit fence through its synchronous native calculation, so sustained churn returns one coherent current native result without panic, stale items, or silent `None`. Direct carrier prop completion must `ensure_loaded` a genuinely cold, disk-only imported `.vue` / `.svelte` child, including when completion starts before `did_open` registration. Root-template ownership is framework-explicit: Vue markup is owned only by an explicit `<template>` block (script-only Vue never borrows Svelte root rules), while Svelte owns carrier-root markup, treats every paired root element/component (including `<template>`) as ordinary markup, and recognizes only `<script>` / `<style>` as SFC blocks. Vue root scaffold snippets never leak at Svelte root whitespace; fail closed until a Svelte-native root producer exists. When Svelte template prop definitions are absent, completion cold-builds through cache-owned component-meta and hydrates a transient child analysis from the cache-owned public contract; preserve authored public keys (aliases, string keys, rest-covered members, named interfaces, and whole-object `$props()`), never infer them from local binding names or source regexes. Component prop label/snippet syntax is owned by the parent carrier language, never inferred from the raw import suffix: extensionless and barrel-resolved Svelte children still use authored keys and `prop={$1}`. Vue attribute syntax is never a fallback for unresolved or zero-public-prop Svelte components; fail closed until a Svelte-native producer owns that surface. Virtual editor projections do not participate in carrier repair generations or lanes, preventing encoded/raw virtual URI identities from splitting lifecycle cleanup.

Provider diagnostics are published only when their generated range maps back to authored source; diagnostics wholly inside synthetic carrier scaffolding remain dropped. If multiple provider reporting sites map to the same current-file identity (range, severity, code, source, and message), the merge publishes one diagnostic and unions tags, related information, code descriptions, and data. Severity is part of identity, so an error and a hint never collapse. Carrier codegen must map the authored reporting token rather than re-anchoring synthetic diagnostics in the merge layer.

### Heartbeat Watchdog

The server sends `$/verter/heartbeat` every 5s from `initialized()`. The VS Code extension monitors heartbeats -- if none arrive for 30s, it auto-restarts the server. Last-resort safety net for runtime starvation.

### Async Workspace Scanning

During `initialized()`, the LSP spawns a `WorkspaceScanner` background task instead of scanning synchronously. The scanner walks the filesystem, compiles `.vue` files to TSX, and syncs them to the type provider in priority order:

1. **Tier 0**: Files opened in the editor (signaled by `did_open`)
2. **Tier 1**: Project source files covered by `tsconfig.json` -- siblings of open files first, then expanding outward
3. **Tier 2**: Remaining `.vue` files not covered by any tsconfig

TSGO sync is throttled (yield every 10 files) to prevent flooding. The scanner receives priority signals from `did_open` to dynamically re-order its queue. Makes `initialized()` return in <1s instead of blocking for the full scan.

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

**Remaining terminal no-serve states:** ONLY `NoProject` (no configured project's include/`files` covers the extension) and the disk-layout carrier-path conflicts (`carrier_never_shadows_real_user_file` / `same_stem_svelte_component_rune_fails_closed`). `NotReady` is the transient bootstrap retry. Terminal `NoProject` / carrier-path conflicts emit a `verter(project)` warning on `did_open` AND `did_change` — `project_ownership_diagnostics_for` (`external_ts/carrier_sync.rs`) is wired into the debounced coordinator's `compute_merged_diagnostics` (`sync_coordinator.rs`), not only the request-only `compute_full_diagnostics`.

**Rename fail-closed for a resolved multi-claimant carrier:** hover / definition / completion / references / diagnostics serve from the single resolved owner, but a provider rename covers only that one project. A symbol that escapes the owner (exported + imported by a sibling configured project) would rename partially; escape detection needs the cross-project fan-out (future block). `handle_rename` / `handle_prepare_rename` therefore FAIL CLOSED (a clear `verter:` error, no `WorkspaceEdit`) via `carrier_is_multi_claimant` (`server/provider_state.rs`) for a resolved multi-claimant carrier — never a silent partial cross-project rename. A uniquely-owned carrier renames normally.

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
