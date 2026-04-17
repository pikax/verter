---
name: host-session
description: "LSP host integration: TypeProvider (TSGO/tsserver), workspace management, async scheduler, SyncCoordinator, ownership lifecycle"
---

# Host & Session

## Language Server Architecture

The LSP is a standalone Rust binary (`verter-lsp`) that communicates with VS Code over stdio.

```
main.rs (stdio transport + CLI args + provider selection)
    |
server.rs (LSP message loop, request dispatch)
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

`ProjectRegistry` groups per-project config for multi-root workspaces. Each `ProjectConfig` has: root path, `TsConfigPathResolver`, `ResolvedLintConfig`, `Linter` instance, and optional `vite_config_path`/`vite_config_deps`. Tsconfig-backed projects use only tsconfig paths; fallback projects get Vite aliases via OXC static analysis (`vite_config.rs`) or trusted Node.js execution.

### TypeProvider Trait (`tsgo/traits.rs`)

Both TSGO and tsserver implement the `TypeProvider` trait. Methods include: hover, completions, diagnostics, definition, references, rename, signature help, code actions, semantic tokens, highlights, inlay hints, open/update/close file, shutdown. The trait is object-safe (`dyn TypeProvider`) so the server is backend-agnostic.

### TSGO Module (`tsgo/`)

| File              | Purpose                                                                                        |
| ----------------- | ---------------------------------------------------------------------------------------------- |
| `ipc.rs`          | LSP client: Content-Length framing, JSON-RPC request/response correlation                      |
| `traits.rs`       | `TypeProvider` trait definition + `TypeProviderError`                                          |
| `protocol.rs`     | Response types: `CompletionResult`, `HoverInfo`, `TypeDiagnostic`, etc.                        |
| `resilient.rs`    | `ResilientTypeProvider`: crash detection via `Notify`, auto-restart (max 3), file cache replay |
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
Request (stdio) -> server.rs -> Find document in host cache -> Feature handler -> Response (stdio)
```

## TypeProvider Architecture

The LSP delegates TypeScript type checking to an external **TypeProvider** process. Two backends are supported:

| Backend      | Binary             | Protocol                                   | Use Case                             |
| ------------ | ------------------ | ------------------------------------------ | ------------------------------------ |
| **TSGO**     | `tsgo` (Go binary) | LSP over stdio (Content-Length + JSON-RPC) | Fast, native TS checking (preview)   |
| **tsserver** | `node tsserver.js` | Newline-delimited JSON over stdio          | Workspace TS version, plugin support |

**tsserver kind mapping**: `parse_tsserver_completion()` in `tsserver/ipc.rs` maps tsserver's `ScriptElementKind` strings to LSP `CompletionItemKind`. This mapping MUST match VS Code's `MyCompletionItem.convertKind()` exactly. Test coverage: `test_parse_tsserver_completion_kinds_match_vscode`. Sync with VS Code source when updating TypeScript dependencies.

**Key modules** (`crates/verter_lsp/src/`):

- `tsgo/` -- TSGO integration (LSP client, resilient wrapper, project sync)
- `tsserver/mod.rs` -- `find_tsserver()`, `find_node()`, `detect_ts_major_version()`
- `tsserver/ipc.rs` -- `TsserverTypeProvider`, newline-delimited JSON transport, position conversion
- `tsserver/resilient.rs` -- `ResilientTsserverProvider` (crash detection + auto-restart)
- `workspace_scanner.rs` -- Async background workspace scanner with priority-based file loading

### Background File Sync

During `initialized()`, the LSP spawns a `WorkspaceScanner` background task that compiles ALL workspace `.vue` files to TSX and syncs them to the type provider asynchronously. For TSGO, both `.vue.tsx` (IDE artifact) and `.vue.ts` (public API) are synced; cross-file imports resolve through `.vue.ts` (via `rewrite_vue_imports_for_tsgo`). This ensures imports of non-open `.vue` files resolve to actual component types rather than the wildcard `declare module '*.vue'` fallback.

### Barrel-Import Eager Sync (TSGO)

When a Vue file imports components through a barrel (non-Vue re-export file like `components/index.ts`), the LSP eagerly syncs the barrel and its Vue dependencies to TSGO during `did_open` and `resync_aliased_imports_for_open_files`. The process: (1) discover barrels from `TemplateComponentUsage.import_source` resolving to non-Vue files, (2) scan barrel's `module_references` for `.vue` specifiers, (3) sync Vue dependencies first, (4) sync barrel file. Without this, TSGO only receives barrels from the background scanner, which may not complete before hover/completion requests.

### Freeze Prevention (Fast Typing)

Three layers prevent tokio runtime starvation during rapid typing:

1. **SyncCoordinator** (`sync_coordinator.rs`): Single long-lived task replaces spawn-per-keystroke debounce. Uses mpsc channel + 300ms deadline map to guarantee exactly one sync per file after typing stops. After syncing, computes and publishes merged (Verter lint + TS type) diagnostics via push. Holds shared `Arc<VerterHost>`, `ProjectSync`, `TypeProvider`, `cached_verter_diags`, and `PositionEncodingKind`.
2. **Push diagnostics only**: The LSP uses push diagnostics exclusively (no pull/`diagnostic_provider`). During typing, no new diagnostics are published -- VS Code automatically adjusts existing push diagnostic positions as the document changes. The SyncCoordinator publishes fresh merged diagnostics after 300ms of silence.
3. **Hang detection** (`tsgo/ipc.rs`): `LspTransport` tracks `consecutive_failures` (AtomicU32). After 3 consecutive request timeouts, fires `crash_notify` to trigger `ResilientTypeProvider`'s existing restart machinery. Notifications use `try_send()` (non-blocking) to prevent channel backpressure.

### Heartbeat Watchdog

The server sends `$/verter/heartbeat` every 5s from `initialized()`. The VS Code extension monitors heartbeats -- if none arrive for 30s, it auto-restarts the server. This is the last-resort safety net for runtime starvation.

### Async Workspace Scanning

During `initialized()`, the LSP spawns a `WorkspaceScanner` background task instead of scanning synchronously. The scanner walks the filesystem, compiles `.vue` files to TSX, and syncs them to the type provider in priority order:

1. **Tier 0**: Files opened in the editor (signaled by `did_open`)
2. **Tier 1**: Project source files covered by `tsconfig.json` -- siblings of open files first, then expanding outward
3. **Tier 2**: Remaining `.vue` files not covered by any tsconfig

TSGO sync is throttled (yield every 10 files) to prevent flooding. The scanner receives priority signals from `did_open` to dynamically re-order its queue. This makes `initialized()` return in <1s instead of blocking for the full scan duration.

**Key module**: `crates/verter_lsp/src/workspace_scanner.rs` -- `WorkspaceScannerHandle`, `spawn_workspace_scanner()`, priority sorting, throttled sync loop.

## Ownership Lifecycle & Bootstrap Sync

The VFS publishes workspace snapshots atomically via `PublishedRoot`. Each snapshot carries an `ownership_ready: bool` flag:

- **Bootstrap** (`ownership_ready: false`): `Engine::new()` eagerly publishes an empty snapshot so basic relative resolution works immediately. Ownership queries return no results. Provider path transforms (`provider_id_for_source`, `provider_ide_id_for_source`) are pure -- they work without ownership.
- **Ready** (`ownership_ready: true`): After `background_init` builds the full project graph, a real snapshot is published. Ownership queries are now authoritative.

**Provider sync state uses typed ownership** (`ProviderOwnerBinding`):

- `Provisional` -- file synced before ownership is known (bootstrap).
- `Owned(String)` -- file bound to a real project (tsconfig path or root).

**Readiness-gated sync rules**:

- `ensure_current_file_synced()`: During bootstrap, provisional sync is allowed. With a ready snapshot, only files with a project owner are synced -- unowned files are queued in `pending_snapshot_provider_sync` for later drain.
- `sync_imported_vue_api_lightweight()`: Same rule -- provisional sync only during bootstrap.
- `SyncCoordinator::sync_file()`: Always queues files with no owner for retry. Uses `ownership_ready` for log level (warn vs info).

**Key files**:

| File | Purpose |
| --- | --- |
| `crates/verter_workspace/src/published_state.rs` | `PublishedRoot`, `ownership_ready` |
| `crates/verter_lsp/src/provider_sync.rs` | `ProviderOwnerBinding`, `ProviderSyncState` |
| `crates/verter_lsp/src/server.rs` | `PublishedResolverSnapshot`, `ensure_current_file_synced` |

## Multi-Root Workspace & Per-Project Configuration

In monorepo / multi-root VS Code workspaces, different packages have different `tsconfig.json` paths aliases, `.verterrc.json` lint rules, and `vite.config` resolve aliases. The LSP stores all workspace folders (`workspace_roots: Mutex<Vec<String>>`) and builds a `ProjectRegistry` that groups per-project configuration.

**Key types** (`crates/verter_lsp/src/config.rs`):

- `ProjectConfig` -- per-project: root path, `ResolvedLintConfig`, `Linter` instance, optional `vite_config_path` and `vite_config_deps`
- `ProjectRegistry` -- sorted by root length (longest prefix first), provides `find_project()`, `find_project_root()`, `linter_for()`
- `RegistryBuildResult` -- returned from `from_workspace_roots()`, contains `registry` + `trust_required` list

**Import resolution** (single VFS authority): All LSP import resolution goes through `WorkspaceAccess::resolve_import()` via the VFS `FilesystemWorkspace`. The workspace is created in `initialize()` with an empty project graph (enabling relative/node_modules resolution immediately), then `background_init` populates the full project graph via `set_project_graph()` for alias resolution. The host's internal `project_resolver` (set via `set_internal_resolver()`) is used only for compilation -- never for LSP resolution. `preferred_specifier()` provides reverse-alias lookup for auto-imports.

**Tsconfig/vite config discovery** delegates to `verter_workspace::config` -- all tsconfig parsing, membership, references, and `raw_paths_json` live in VFS. Fallback projects (no tsconfig) get Vite aliases via two-tier analysis in `vite_config.rs`:

1. **Static analysis** (OXC): Parses `vite.config.{ts,js,mjs,cjs,mts,cts}` without executing code. Handles object/array alias forms, `defineConfig()`, template literals, `path.resolve()`, `new URL()`, `fileURLToPath()`. Returns `Complex` for configs using env vars, dynamic imports, or non-allowlisted packages.
2. **Trusted execution** (opt-in): For complex configs, spawns Node.js with `loadConfigFromFile` if the file is in `verter.viteConfig.trustedFiles`. Includes env sanitization, 10s timeout, and last-known-good caching.

The server sends `$/verter/viteConfigTrustRequired` notifications for complex configs not yet trusted, and the extension shows a trust prompt. Config file changes (detected via file watcher) trigger a full registry rebuild.

**Type provider integration**: TSGO receives `workspace/didChangeWorkspaceFolders` notifications. tsserver uses per-file `projectRootPath` from the project registry. Both resilient wrappers store workspace folders for restart replay.

**Lock ordering** (prevents deadlocks): `workspace_roots` (async) -> `project_registry` (sync read) -> release -> `fallback_linter` (sync read). Never acquire `fallback_linter` while holding `project_registry`.

## Async File Scheduler (`verter_scheduler`)

The scheduler provides per-file async staging with priority queuing. Files progress independently through **Source -> Analysis -> Artifact** stages. Cross-file blocking (macro type deps, external `src`) is declarative -- the scheduler manages wakeups.

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

Feature-gated (`scheduler`): `VerterHost` holds an `Arc<Scheduler>`. During `upsert()`, the host submits to the scheduler, awaits the `CompletionHandle`, reads back the result, and populates the compile cache. The `HostStageExecutor` calls real `parse_vue_snapshot`/`parse_non_sfc_snapshot` for the Source stage. Host-specific data is stored in snapshots via the `SnapshotData` trait (opaque `Arc<dyn Any>`), avoiding circular dependencies between scheduler and host.

### LSP Integration

`scheduler_integration.rs` maps LSP operations to priority tiers (Critical for hover/completion, Interactive for did_open/change, Background for workspace scan). `compile_blockers.rs` is deprecated -- the scheduler's blocker model replaces imperative hydration.

### Authority Chain (Final State)

1. **Scheduler** = sole parser, raw source + analysis authority (`HostSourceData`, `HostAnalysisData`). `HostSourceData::source_type` is the authoritative `oxc_span::SourceType` for downstream cache-key sites -- computed once at `execute_source` time with full access to the parsed SFC. Cache-key callers read via `VerterHost::authoritative_source_type_for(canonical)` or the higher-level `imported_eval_source_type_for(...)` helper.
2. **compile_cache** (`DashMap`) = profile state authority (compile_slots, overrides, diagnostics, deps, resolved_type_hashes). `CompileCacheEntry.evicted_whole_hash: Option<Hash16>` carries the pre-evict hash; `ensure_loaded` compares it to the post-reload hash and skips `bump_store_view_epoch` on no-op reloads so thread-local caches stay warm.
3. **files** (`Shared<FxHashMap>`) = WASM-only primary store. Not used on native (scheduler) path. Gated `#[cfg(target_arch = "wasm32")]` / `#[cfg(any(target_arch = "wasm32", test))]` after the Phase 1 cleanup.

### Request-Scoped View Contract

`RequestStoreView` (`crates/verter_session/src/host_request_view.rs`) is the single per-request view captured at request entry. It wraps the host-owned `HostStoreView` snapshot plus a request-private `RequestExtension` map covering `whole_hash`, derived hashes (`Route`, `ImportRoute`), and `import_route` resolutions for canonicals discovered mid-request via `ensure_loaded`.

Key rules:

- Resolvers inside a request consult the request view for all "what does the host know about this canonical?" lookups. Live `host.scheduler.try_get_source(...)`, `module_facts.get_any(...)`, `host.get_whole_hash(...)` probes inside resolver paths are considered view-bypass.
- `RequestStoreView::is_evalable(canonical)` is the canonical shallow-probe API for feasibility checks. `VerterHost::is_evalable(canonical)` reads from the current request view via the thread-local `CURRENT_REQUEST_VIEW`, falling back to the cheap `get_whole_hash` check outside of any request.
- Mid-request `ensure_loaded` integration uses the thread-local `CURRENT_REQUEST_VIEW` guard to record extensions via `VerterHost::record_current_request_extension_for`; top-level callers see no plumbing change.
- The request-scoped `external_inputs_memo` on `RequestStoreView` caches the *result of fetching from* `external_type_analysis_cache` / `module_facts`, keyed by `(canonical_id, whole_hash)`. It is a lookup memo, NOT a parallel parser path -- the host-scoped cache remains the single source of truth for `Arc<AnalyzedExternalTypeSource>`.
- View staleness during long-running requests (Editor menus, multi-second queries) is bounded -- the captured view does not see post-snapshot upserts; the next request snapshots fresh.

**Phase 1 scope carried forward to follow-up work**: the full signature rewrite of every `pub(crate) fn *_in_view(..., store_view: Option<&HostStoreView>)` to `view: &RequestStoreView` is a mechanical but wide-blast-radius change (~128 sites). Until it lands, callers keep the `Option<&HostStoreView>` signature and reach the extension store via the thread-local; the trace-gate budgets (Accordion `resolve_imported_type_root ≤ 2`, `cached_type_resolution_context_hit ≥ 80 %`) only tighten once signatures thread `&RequestStoreView` directly.

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
| `crates/verter_lsp/src/server.rs` | LSP message loop, request dispatch |
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
