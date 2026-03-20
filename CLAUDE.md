# Verter

Verter is a Vue compiler and Language Server Protocol (LSP) implementation. It converts Vue Single File Components (SFCs) to valid TSX (leveraging TypeScript for type checking) and compiles templates to optimized render functions. Unlike Volar, Verter generates actual valid TSX code rather than virtual files.

The project is a hybrid Rust + TypeScript monorepo: Rust crates handle template compilation (exposed via NAPI-RS native bindings and wasm-bindgen WASM) and the LSP server (`verter_lsp` binary, communicates over stdio), while TypeScript packages handle the SFC-to-TSX transformation and IDE integration.

## Architecture

### Shared Optimized Codebase (CRITICAL)

Verter is one shared optimized codebase, not separate semantic implementations per consumer.

- Improvements should land in the lowest reusable owner crate that can correctly serve all consumers.
- `verter_host` and shared workspace/VFS integration are the authority for host-backed loading, invalidation, dependency tracking, and cache reuse.
- `verter_analysis` and `verter_core` own reusable semantics and type-resolution logic.
- `verter_ffi` owns shared boundary DTOs between native bindings and WASM bindings.
- Consumer packages such as `@verter/component-meta`, the LSP, MCP, unplugin, and playground should consume the shared substrate rather than carrying their own semantic forks.

Architectural consequence:

- A performance or correctness fix discovered in one surface should be implemented in the shared owner layer whenever that behavior is reusable.
- Consumer-local wrappers should stay thin and should not bypass shared parsing, analysis, resolution, or cache ownership.

### Package Dependency Graph

```
verter-vscode (VS Code extension)
├── verter-lsp (Rust LSP binary, stdio)
│   ├── verter_host (file host + compilation)
│   │   └── verter_scheduler (async per-file staging, feature-gated)
│   ├── verter_scheduler (priority queue, completion handles)
│   ├── verter_diagnostics (lint rules + DiagnosticSet)
│   ├── verter_actions (quick fixes + refactoring)
│   └── TypeProvider (optional: TSGO or tsserver, for TS type checking)
├── @verter/language-shared (custom protocol types)
├── @verter/typescript-plugin (.vue import resolution, NAPI-backed)
└── @verter/unplugin (bundler plugin)
    └── @verter/native

verter-mcp (MCP server binary, stdio + HTTP)
├── verter_host (file host + compilation)
├── verter_analysis (static analysis snapshots)
├── verter_diagnostics (lint rules + DiagnosticSet)
└── verter_actions (quick fixes + refactoring)

@verter/playground (Netlify-hosted)
└── @verter/wasm (Rust template compiler, wasm-bindgen)

@verter/component-meta (metadata extraction)
├── @verter/native (NAPI host, Node.js)
└── @verter/wasm (WASM host, browser, optional)
```

### Repository Structure

```
crates/
  verter_core/       # Core template compiler (Rust)
  verter_analysis/   # Static analysis: imports, exports, bindings, type resolution
  verter_host/       # In-memory file host: caching, dependency tracking, multi-file compilation
  verter_scheduler/  # Async per-file scheduler: Source→Analysis→Artifact stages, priority queue, blocker registry
  verter_diagnostics/ # Vue SFC diagnostic engine: ~186 lint rules, rule trait, visitor, DiagnosticSet (depends only on verter_analysis)
  verter_actions/    # Code actions engine: quick fixes, refactoring (depends on verter_diagnostics + verter_analysis)
  verter_lsp/        # Rust LSP server binary (stdio, launched by VS Code extension)
  verter_ffi/        # FFI types: shared serializable structs for NAPI/WASM boundaries
  verter_bench/      # Benchmarks and comparison examples (Rust)
  verter_mcp/        # MCP server binary: analysis, diagnostics, scoring for AI agents
  verter_napi/       # Native Node.js bindings (NAPI-RS cdylib)
  verter_wasm/       # WASM bindings (wasm-bindgen cdylib)
packages/
  core/              # @verter/core - SFC parser & TSX transformer
  types/             # @verter/types - TypeScript utility types
  native/            # @verter/native - Native binding loader + platform packages
  wasm/              # @verter/wasm - WASM binding wrapper
  unplugin/          # @verter/unplugin - Universal bundler plugin
  language-shared/   # @verter/language-shared - Shared LSP protocol types
  typescript-plugin/ # @verter/typescript-plugin - TS language service plugin
  oxc-bindings/      # @verter/oxc-bindings - OXC parser binary helper
  component-meta/    # @verter/component-meta - Component metadata extraction + Type IR + adapters
  playground/        # @verter/playground - Online playground (private, Netlify-hosted)
  vue-vscode/        # verter-vscode - VS Code extension
  example/           # Example project
scripts/
  check-versions.mjs # Version check + publish order for CI
```

### Async File Scheduler (`verter_scheduler`)

The scheduler provides per-file async staging with priority queuing. Files progress independently through **Source → Analysis → Artifact** stages. Cross-file blocking (macro type deps, external `src`) is declarative — the scheduler manages wakeups.

**Key types** (`crates/verter_scheduler/src/`):
- `FileNode` (`node.rs`) — per-file: ArcSwap snapshots, AtomicU64 generation, pending requests
- `Scheduler` (`scheduler.rs`) — DashMap of FileNodes, JobIndex, SubmissionInbox, driver thread
- `CompletionHandle<T>` (`job.rs`) — request-scoped, resolves to Ready/Failed/Superseded/Shutdown
- `StageExecutor` (`executor.rs`) — trait for plugging host parse/analysis/compile logic
- `Priority` (`stage.rs`) — 4 tiers: Critical > Interactive > Background > Maintenance
- `EdgeManager` (`edges.rs`) — ReverseIndex + BlockerRegistry (both DashMap-sharded)
- `OverlayMap` (`overlay.rs`) — concurrent editor buffer storage (DashMap)
- `SourceLoader` (`source_loader.rs`) — MemorySourceLoader (tests/WASM) + DiskSourceLoader (native)
- `IoPool` (`pool.rs`) — bounded I/O thread pool, separate from rayon CPU pool

**Snapshot model**: All stage outputs are immutable `Arc`-wrapped. Replacement is atomic via ArcSwap. Generation fencing ensures coherence: `source.gen == analysis.gen == artifact.gen == node.gen`. Stale snapshots (gen mismatch) return `None` from `current_*()` methods.

**Host integration** (feature-gated `scheduler`): `VerterHost` holds an `Arc<Scheduler>`. During `upsert()`, the host populates both the legacy `files: Shared<FxHashMap>` and the scheduler in parallel. The `HostStageExecutor` calls real `parse_vue_snapshot`/`parse_non_sfc_snapshot` for the Source stage. Host-specific data is stored in snapshots via the `SnapshotData` trait (opaque `Arc<dyn Any>`), avoiding circular dependencies between scheduler and host.

**LSP integration**: `scheduler_integration.rs` maps LSP operations to priority tiers (Critical for hover/completion, Interactive for did_open/change, Background for workspace scan). `compile_blockers.rs` is deprecated — the scheduler's blocker model replaces imperative hydration.

### Two Template Codegen Paths (CRITICAL)

The Rust compiler has **two separate template codegen paths**. Modifying one does NOT affect the other:

| Path | Module | Purpose | Output |
|------|--------|---------|--------|
| **VDOM/Vapor** | `template/code_gen/vdom/` | Runtime render functions for bundler output | `_createElementVNode(...)` calls |
| **IDE** | `ide/template/` | Valid JSX/TSX for LSP/TSGO type checking | `<div prop={expr}>` JSX elements |

The **LSP uses the IDE path** via `host.ensure_compiled()` with `CompileTarget::IDE`. TSGO type-checks this output. Changes to VDOM codegen do NOT affect LSP hover/completions. The IDE codegen auto-detects the script language: TS SFCs produce `.tsx` (TypeScript + JSX), while JS SFCs (no `lang` or `lang="js"`) produce `.jsx` (JavaScript + JSDoc annotations).

### Strict Slot Children Type Checking (Experimental)

When `strict_slots: true` (VS Code: `verter.experimental.strictSlots`), the IDE template codegen emits `strictRenderSlot` calls after the JSX tree. These enforce that slot children match the parent component's `defineSlots()` type signature ([RFC #733](https://github.com/vuejs/rfcs/discussions/733)).

**Generated pattern** (inside the block scope, after JSX):
```tsx
___VERTER___strictRenderSlot({} as NonNullable<ReturnType<typeof ___VERTER___Comp{offset}>['$slots']['{slot}']>, [TabItem, {} as HTMLElementTagNameMap["input"], "" as string]);
```

**Child type references**: Component → constructor name, HTML element → `HTMLElementTagNameMap["tag"]`, text/interpolation → `"" as string`. Each child is a sourcemapped `InsertedMapped` chunk pointing to its template position.

**Skipped cases**: self-closing components (no children), `is_jsx` mode, `<component :is>` (deferred), whitespace-only text, comments.

**Key files**: `ide/template/mod.rs` (`StrictSlotEntry`, `collect_strict_slot_children`, `emit_strict_slot_checks`), `ide/script.rs` (ambient `strictRenderSlot` type declarations).

### CodeTransform Is the Single Source of Truth (CRITICAL)

**All modifications to generated code MUST go through `CodeTransform` operations** (`overwrite`, `prepend_left`, `append_left`, `move_with_suffix`, etc.). Never apply string replacements, regex transforms, or manual splicing to the output of `build_string()` or to content that was produced by a `CodeTransform`.

Post-hoc string manipulation breaks sourcemap accuracy: the `CodeTransform` generates source maps by tracking chunks (Original, Inserted, Moved, Overwritten). If you modify the string after the transform, byte offsets in the source map no longer match the actual content. This causes position mismatches in the LSP (e.g., hover landing on the wrong token, go-to-definition jumping to wrong locations).

**Correct:** Use `ct.prepend_left(pos, ".ts")` to insert text at a known position — the chunk list and source map stay consistent.

**Wrong:** Call `content.replace(".vue'", ".vue.ts'")` on the built string — the source map still reflects the pre-replace byte offsets.

### IDE Script Error Recovery

When OXC encounters parse errors during typing (e.g., `count.` mid-expression), the IDE script codegen (`ide/script.rs`) uses a **truncate-and-reparse** strategy instead of falling back to degraded file-scope output:

1. Find the earliest error offset from OXC diagnostics.
2. Truncate source at the last newline before that offset — the "clean prefix".
3. Re-parse only the clean prefix (which succeeds since the broken code is removed).
4. Use the clean prefix AST for normal codegen (import hoisting, binding extraction, macro processing). The broken tail passes through unchanged in the CodeTransform.

A lightweight token scanner (`ide/script_recover.rs`) recovers macro binding names from the broken tail so template bindings still resolve. This means typing `count.` at the end of a script preserves hover, completions, and go-to-definition for all declarations above the cursor.

**Fallback**: When the clean prefix is empty (error on first line) or the clean prefix itself fails to parse, the system falls back to file-scope error recovery mode (`process_tsx_script_setup_error_mode`).

### TypeProvider Architecture

The LSP delegates TypeScript type checking to an external **TypeProvider** process. Two backends are supported:

| Backend | Binary | Protocol | Use Case |
|---------|--------|----------|----------|
| **TSGO** | `tsgo` (Go binary) | LSP over stdio (Content-Length + JSON-RPC) | Fast, native TS checking (preview) |
| **tsserver** | `node tsserver.js` | Newline-delimited JSON over stdio | Workspace TS version, plugin support |

**Provider selection** (`--type-provider` CLI arg / `verter.typeProvider` VS Code setting):
- `auto` (default): if TS 5.x/6.x installed, uses tsserver; otherwise tries TSGO
- `tsgo`: TSGO only
- `tsserver`: tsserver only
- `off`: no type provider (verter-only mode)

Only one provider runs at a time. Both use the `TypeProvider` trait (`tsgo/traits.rs`) with 14+ methods (hover, completions, diagnostics, definition, references, rename, etc.). Both are wrapped in a `ResilientTypeProvider` that detects crashes, auto-restarts (max 3 with exponential backoff), and replays the file cache.

**tsserver kind mapping**: `parse_tsserver_completion()` in `tsserver/ipc.rs` maps tsserver's `ScriptElementKind` strings to LSP `CompletionItemKind`. This mapping MUST match VS Code's `MyCompletionItem.convertKind()` exactly. Test coverage: `test_parse_tsserver_completion_kinds_match_vscode`. Sync with VS Code source when updating TypeScript dependencies.

**Key modules** (`crates/verter_lsp/src/`):
- `tsgo/` — TSGO integration (LSP client, resilient wrapper, project sync)
- `tsserver/mod.rs` — `find_tsserver()`, `find_node()`, `detect_ts_major_version()`
- `tsserver/ipc.rs` — `TsserverTypeProvider`, newline-delimited JSON transport, position conversion
- `tsserver/resilient.rs` — `ResilientTsserverProvider` (crash detection + auto-restart)
- `workspace_scanner.rs` — Async background workspace scanner with priority-based file loading

**Background file sync**: During `initialized()`, the LSP spawns a `WorkspaceScanner` background task that compiles ALL workspace `.vue` files to TSX and syncs them to the type provider asynchronously. For TSGO, both `.vue.tsx` (IDE artifact) and `.vue.ts` (public API) are synced; cross-file imports resolve through `.vue.ts` (via `rewrite_vue_imports_for_tsgo`). This ensures imports of non-open `.vue` files resolve to actual component types rather than the wildcard `declare module '*.vue'` fallback.

**Barrel-import eager sync** (TSGO): When a Vue file imports components through a barrel (non-Vue re-export file like `components/index.ts`), the LSP eagerly syncs the barrel and its Vue dependencies to TSGO during `did_open` and `resync_aliased_imports_for_open_files`. The process: (1) discover barrels from `TemplateComponentUsage.import_source` resolving to non-Vue files, (2) scan barrel's `module_references` for `.vue` specifiers, (3) sync Vue dependencies first, (4) sync barrel file. Without this, TSGO only receives barrels from the background scanner, which may not complete before hover/completion requests.

**Freeze prevention** (fast typing): Three layers prevent tokio runtime starvation during rapid typing:
1. **SyncCoordinator** (`sync_coordinator.rs`): Single long-lived task replaces spawn-per-keystroke debounce. Uses mpsc channel + 300ms deadline map to guarantee exactly one sync per file after typing stops. After syncing, computes and publishes merged (Verter lint + TS type) diagnostics via push. Holds shared `Arc<VerterHost>`, `ProjectSync`, `TypeProvider`, `cached_verter_diags`, and `PositionEncodingKind`.
2. **Push diagnostics only**: The LSP uses push diagnostics exclusively (no pull/`diagnostic_provider`). During typing, no new diagnostics are published — VS Code automatically adjusts existing push diagnostic positions as the document changes. The SyncCoordinator publishes fresh merged diagnostics after 300ms of silence.
3. **Hang detection** (`tsgo/ipc.rs`): `LspTransport` tracks `consecutive_failures` (AtomicU32). After 3 consecutive request timeouts, fires `crash_notify` to trigger `ResilientTypeProvider`'s existing restart machinery. Notifications use `try_send()` (non-blocking) to prevent channel backpressure.

**Heartbeat watchdog**: The server sends `$/verter/heartbeat` every 5s from `initialized()`. The VS Code extension monitors heartbeats — if none arrive for 30s, it auto-restarts the server. This is the last-resort safety net for runtime starvation.

**Async workspace scanning**: During `initialized()`, the LSP spawns a `WorkspaceScanner` background task instead of scanning synchronously. The scanner walks the filesystem, compiles `.vue` files to TSX, and syncs them to the type provider in priority order:

1. **Tier 0**: Files opened in the editor (signaled by `did_open`)
2. **Tier 1**: Project source files covered by `tsconfig.json` — siblings of open files first, then expanding outward
3. **Tier 2**: Remaining `.vue` files not covered by any tsconfig

TSGO sync is throttled (yield every 10 files) to prevent flooding. The scanner receives priority signals from `did_open` to dynamically re-order its queue. This makes `initialized()` return in <1s instead of blocking for the full scan duration.

**Key module**: `crates/verter_lsp/src/workspace_scanner.rs` — `WorkspaceScannerHandle`, `spawn_workspace_scanner()`, priority sorting, throttled sync loop.

### Ownership Lifecycle & Bootstrap Sync

The VFS publishes workspace snapshots atomically via `PublishedRoot`. Each snapshot carries an `ownership_ready: bool` flag:

- **Bootstrap** (`ownership_ready: false`): `Engine::new()` eagerly publishes an empty snapshot so basic relative resolution works immediately. Ownership queries return no results. Provider path transforms (`provider_id_for_source`, `provider_ide_id_for_source`) are pure — they work without ownership.
- **Ready** (`ownership_ready: true`): After `background_init` builds the full project graph, a real snapshot is published. Ownership queries are now authoritative.

**Provider sync state uses typed ownership** (`ProviderOwnerBinding`):
- `Provisional` — file synced before ownership is known (bootstrap).
- `Owned(String)` — file bound to a real project (tsconfig path or root).

**Readiness-gated sync rules**:
- `ensure_current_file_synced()`: During bootstrap, provisional sync is allowed. With a ready snapshot, only files with a project owner are synced — unowned files are queued in `pending_snapshot_provider_sync` for later drain.
- `sync_imported_vue_api_lightweight()`: Same rule — provisional sync only during bootstrap.
- `SyncCoordinator::sync_file()`: Always queues files with no owner for retry. Uses `ownership_ready` for log level (warn vs info).

**Key files**: `crates/verter_vfs/src/published_state.rs` (`PublishedRoot`, `ownership_ready`), `crates/verter_lsp/src/provider_sync.rs` (`ProviderOwnerBinding`, `ProviderSyncState`), `crates/verter_lsp/src/server.rs` (`PublishedResolverSnapshot`, `ensure_current_file_synced`).

### Multi-Root Workspace & Per-Project Configuration

In monorepo / multi-root VS Code workspaces, different packages have different `tsconfig.json` paths aliases, `.verterrc.json` lint rules, and `vite.config` resolve aliases. The LSP stores all workspace folders (`workspace_roots: Mutex<Vec<String>>`) and builds a `ProjectRegistry` that groups per-project configuration.

**Key types** (`crates/verter_lsp/src/config.rs`):
- `ProjectConfig` — per-project: root path, `ResolvedLintConfig`, `Linter` instance, optional `vite_config_path` and `vite_config_deps`
- `ProjectRegistry` — sorted by root length (longest prefix first), provides `find_project()`, `find_project_root()`, `linter_for()`
- `RegistryBuildResult` — returned from `from_workspace_roots()`, contains `registry` + `trust_required` list

**Import resolution** (single VFS authority): All LSP import resolution goes through `WorkspaceAccess::resolve_import()` via the VFS `FilesystemWorkspace`. The workspace is created in `initialize()` with an empty project graph (enabling relative/node_modules resolution immediately), then `background_init` populates the full project graph via `set_project_graph()` for alias resolution. The host's internal `project_resolver` (set via `set_internal_resolver()`) is used only for compilation — never for LSP resolution. `preferred_specifier()` provides reverse-alias lookup for auto-imports.

**Tsconfig/vite config discovery** delegates to `verter_vfs::config` — all tsconfig parsing, membership, references, and `raw_paths_json` live in VFS. Fallback projects (no tsconfig) get Vite aliases via two-tier analysis in `vite_config.rs`:
1. **Static analysis** (OXC): Parses `vite.config.{ts,js,mjs,cjs,mts,cts}` without executing code. Handles object/array alias forms, `defineConfig()`, template literals, `path.resolve()`, `new URL()`, `fileURLToPath()`. Returns `Complex` for configs using env vars, dynamic imports, or non-allowlisted packages.
2. **Trusted execution** (opt-in): For complex configs, spawns Node.js with `loadConfigFromFile` if the file is in `verter.viteConfig.trustedFiles`. Includes env sanitization, 10s timeout, and last-known-good caching.

The server sends `$/verter/viteConfigTrustRequired` notifications for complex configs not yet trusted, and the extension shows a trust prompt. Config file changes (detected via file watcher) trigger a full registry rebuild.

**Type provider integration**: TSGO receives `workspace/didChangeWorkspaceFolders` notifications. tsserver uses per-file `projectRootPath` from the project registry. Both resilient wrappers store workspace folders for restart replay.

**Lock ordering** (prevents deadlocks): `workspace_roots` (async) → `project_registry` (sync read) → release → `fallback_linter` (sync read). Never acquire `fallback_linter` while holding `project_registry`.

### Style Preprocessing in Bundler Mode

Style blocks with `lang="scss"`, `lang="sass"`, or `lang="less"` require preprocessing to CSS. The pipeline differs between Vite and non-Vite bundlers:

**Vite mode** (Vite-owned preprocessing, matching `@vitejs/plugin-vue`):
1. During main `.vue` `transform()`, the plugin parses the SFC with `compiler.parse()` and caches raw style block content in `styleBlockCache`. Style preprocessing is **skipped** in `applyPreprocessorRequests()`.
2. `load()` returns raw style source (e.g., SCSS with `$variables`) from `styleBlockCache`.
3. Style URLs preserve the original lang (`lang.scss`, not `lang.css`) since `meta.style_langs` is never overwritten.
4. Vite's CSS pipeline preprocesses SCSS/SASS/Less/Stylus automatically between `load()` and `transform()`.
5. `transform()` always runs `compiler.compileStyleAsync()` for Vue-specific post-processing: scoped CSS attribute selectors (`[data-v-...]`) and CSS `v-bind()` rewriting. This runs even for unscoped plain CSS blocks (CSS `v-bind()` still needs rewriting).

**Non-Vite mode** (preprocessor fallback):
Style preprocessing goes through `preprocessBlock()` → `preprocessStyle()` which calls Vite's `preprocessCSS()` in-process (if Vite config is available). The compiled CSS is sent to the Rust host via `applyBlockOverrides()`, and `apply_style_overrides()` updates `meta.style_langs` to `"css"`. The `transform()` hook uses Rust `processStyle()` for CSS scoping only.

**Compiler resolution**: `vue/compiler-sfc` is resolved once per plugin instance from the project root in `configResolved()` via `createRequire(join(root, "package.json"))("vue/compiler-sfc")`. This is stored in the `compiler` variable and used for both SFC parsing (`compiler.parse()`) and style post-processing (`compiler.compileStyleAsync()`).

**Key files**: `packages/unplugin/src/index.ts` (`styleBlockCache`, `compileStyleAsync` in transform, style load from cache), `packages/unplugin/src/core/preprocessor.ts` (non-Vite style preprocessing via `preprocessStyle()`), `crates/verter_host/src/host_upsert.rs` (`apply_style_overrides` — lang update, non-Vite only), `crates/verter_host/src/id.rs` (`render_ids` — URL generation).

### Cached Directive Fields on ElementNode

The parser extracts structural directives from `el.props` via `prop.take()` and caches them as dedicated fields on `ElementNode` (`ast/types.rs`):

| Field | Directive | In `el.props`? | Notes |
|-------|-----------|----------------|-------|
| `v_condition` | `v-if`, `v-else-if`, `v-else` | **No** (taken) | Contains `ElementNodeCondition` with kind + prop |
| `v_for` | `v-for` | **No** (taken) | Contains the full `NodeProp` |
| `v_slot` | `v-slot`, `#name` | **No** (taken) | Contains the full `NodeProp` |
| `v_once` | `v-once` | **No** (taken) | Contains the full `NodeProp` |
| `v_ref` | `ref`, `:ref` | **No** (taken) | Contains the full `NodeProp` |

**Consequence**: Code iterating `el.props` will **never see** these directives. Both codegen paths must handle them explicitly. The IDE module removes `v-if/v-for/v-slot/v-once` attributes (they become JSX wrappers/removals) and converts `ref` to JSX expression syntax (`ref={"name"}`).

### Position Encoding (CRITICAL rules)

See `/position-encoding` skill for full span type reference, encoding tables, and path normalization details.

**Encoding source of truth**: The position encoding MUST come from the client capabilities negotiated during `initialize()`. The server stores it in `Arc<parking_lot::RwLock<PositionEncodingKind>>` shared with the SyncCoordinator. Default is UTF-16 (per LSP spec) until negotiated. **Rust-internal code uses UTF-8 byte offsets**; **LSP boundary code converts to negotiated encoding**; **JS/VS Code uses UTF-16**.

**Line/Column Base Rules** (off-by-one bugs):
- **PositionResolver is 1-based** — subtract 1 for source maps and LSP
- **Source maps, LSP, VS Code are all 0-based**
- **OXC/verter spans are byte offsets** — no line/column conversion needed

**Serialization rule**: All data crossing serde/MCP/LSP/FFI boundaries MUST use `Span` (SFC-absolute). `RelativeSpan`/`PartialGeneratedSpan`/`GeneratedSpan` do not implement Serialize.

### Path Normalization (CRITICAL rules)

See `/position-encoding` skill for canonical ID format and boundary tables.

1. **Receive → normalize immediately** (`canonicalize_id()` or `uri_to_canonical_id_from_str()`)
2. **Store only canonical IDs** in all maps and caches
3. **Denormalize at exit boundaries** (file:// URIs or OS paths)
4. **Never compare raw paths** — always compare canonical IDs

## Build

```bash
pnpm install                  # Install all dependencies
pnpm build                    # Build everything: native → lsp → wasm → ts packages
pnpm run build:native         # Build native .node bindings only
pnpm run build:lsp            # Build Rust LSP binary (debug)
pnpm run build:lsp:release    # Build Rust LSP binary (release, optimized)
pnpm run build:mcp            # Build MCP server binary (debug)
pnpm run build:mcp:release    # Build MCP server binary (release, optimized)
pnpm run build:wasm           # Build WASM + copy to playground
pnpm run build:ts             # Build all TypeScript packages
pnpm run build:playground     # Build the playground for deployment
```

`pnpm build` runs sequentially: native bindings first (needed by unplugin), then LSP binary (shares compiled Rust deps with native, avoids recompilation), then WASM (needed by playground), then all TS packages.

See `/build-and-profiling` skill for build dependency chains, rebuild sequences, and profiling setup.

## Development

```bash
pnpm watch                    # Watch-build TS packages for extension dev
pnpm dev-extension            # Build LSP binary, then watch language-shared + vscode extension + typescript-plugin
pnpm clean                    # Remove build artifacts
```

## Testing

### Running Tests

```bash
# TypeScript / JavaScript
pnpm test                                    # All JS/TS tests
pnpm vitest --run                            # All tests (non-watch)
pnpm vitest --run path/to/test.spec.ts       # Specific file

# Rust
cargo test --workspace --verbose             # All Rust tests
cargo test --package verter_core test_name   # Specific Rust test
cargo test --package verter_core 2>&1 | tail -60  # Full suite with truncated output
```

### End-of-change Checks

Run these after making changes:

```bash
cargo clippy --fix --allow-dirty --allow-staged --workspace -- -D warnings
cargo fmt --all
pnpm install --frozen-lockfile   # Verify lockfile is in sync (CI uses this)
```

### Documentation Updates

After adding, changing, or removing features, check and update relevant documentation:

- **`CLAUDE.md`** — Architecture tables, module paths, key file references
- **`docs/`** — API docs, guide pages, contributing guides (`docs/contributing/rust-setup.md`, etc.)
- **`.claude/skills/`** — Skill files referencing affected modules or APIs
- **Inline doc comments** — Public API rustdoc (`///`) and JSDoc (`/** */`) on changed signatures

Skip this for purely internal refactors that don't change any public behavior, module paths, or APIs.

### Testing Requirements

**MANDATORY RULE — TDD (Test-Driven Development) must be followed for EVERY code change. This is non-negotiable. All agents, subagents, and automated workflows MUST comply. Skipping TDD is never acceptable, regardless of task size or urgency.**

**TDD workflow (strict order — no exceptions):**
1. **Write failing tests FIRST** — before writing ANY implementation code, write one or more tests that demonstrate the expected behavior. Run the tests and **verify they fail**. Do not proceed to step 2 until you have confirmed test failure.
2. **Implement the minimum code** to make the failing tests pass. Do not write implementation code before tests exist.
3. **Run the tests again** and verify they pass.
4. **Refactor** if needed while keeping tests green.

**Violation examples (DO NOT do these):**
- Writing implementation code and then adding tests after the fact
- Writing tests and implementation simultaneously without verifying the tests fail first
- Skipping tests for "small" or "trivial" changes
- Delegating implementation to a subagent without requiring TDD compliance

Coverage expectations:
- New features: Add tests covering the new functionality
- Bug fixes: Add tests that would have caught the bug
- Refactoring: Ensure existing tests pass and add tests for edge cases discovered
- Behavioral changes: Add tests verifying the new behavior

Tests serve as documentation of expected behavior and prevent regressions.

**IMPORTANT — Always include negative assertions**:

Every test must verify both what SHOULD be present AND what should NOT be present. A test that only checks for expected output can pass even when the output contains invalid/broken content alongside the expected content.

```rust
// GOOD: Both positive and negative assertions
let result = gen_tsx_template(r#"<template><div v-if="show">hello</div></template>"#);
assert!(result.contains("if(show)"), "should have IIFE if-block condition");  // positive
assert!(!result.contains("v-if"), "v-if attribute must be removed from JSX"); // negative

// BAD: Only positive assertion — passes even if v-if="show" leaks into output
let result = gen_tsx_template(r#"<template><div v-if="show">hello</div></template>"#);
assert!(result.contains("if(show)"), "should have IIFE if-block condition"); // not enough!
```

For codegen tests: always verify that removed/transformed Vue syntax does NOT appear in output. For type tests: always include both positive assertions and `@ts-expect-error` negative assertions to guard against `any`/`never`.

**IMPORTANT — Rust test file organization**:

When a Rust source file's inline `#[cfg(test)] mod tests` block exceeds ~400 lines, extract tests to a sibling `*_tests.rs` file:

```rust
// In foo.rs — replace the inline mod tests block with:
#[cfg(test)]
#[path = "foo_tests.rs"]
mod foo_tests;
```

For `mod.rs` files, use the simpler form (loads `tests.rs` from the same directory):

```rust
#[cfg(test)]
mod tests;
```

The extracted file contains the module contents directly (no wrapping `mod tests { }`), starting with `use super::*;`.

See `/testing` skill for full TS/Rust test patterns, sourcemap testing, E2E best practices, and server cleanup.

### VS Code Extension Testing (MANDATORY)

Changes to the VS Code extension (`packages/vue-vscode/`) or the LSP server (`crates/verter_lsp/`) MUST be verified with automated tests, NOT manual testing. LSP changes directly affect extension behavior — hover, completions, diagnostics, etc. Two test tiers exist:

**Unit tests** (Vitest, `*.spec.ts` co-located in `src/`):
- For pure logic: utility functions, response parsing, restart logic, CSS scanning
- Run: `pnpm vitest --run packages/vue-vscode/src/path/to/file.spec.ts`

**E2E tests** (Mocha + @vscode/test-cli, `e2e/suite/*.test.ts`):
- For LSP integration: completions, hover, diagnostics, go-to-definition, rename, decorations
- Single fixture/provider: `E2E_FIXTURE=single-project E2E_TYPE_PROVIDER=tsserver pnpm --filter verter-vscode test:e2e`
- Full matrix from the repo root: `pnpm run test:e2e`
- CI runs the same fixture matrix across both `tsserver` and `tsgo`
- See `.claude/skills/e2e-vscode-testing.md` for fixture design, helpers API, and adding new tests

**When to use which:**
- New extension utility/parser logic → unit test
- New/changed LSP feature (hover, completion, diagnostics, definition, rename, decorations) → E2E test
- `verter_lsp` changes (new handler, changed response format, sync behavior) → E2E test
- Both if the change spans utility logic + LSP behavior

**Never acceptable:** "Test manually by opening VS Code" as the sole verification step in a plan.

### Agent Feedback Capture

During work sessions, agents encounter issues, discover improvement opportunities, and gain insights that may be lost when context is compacted. To preserve these observations, agents MUST continuously log feedback to a per-conversation file.

**Setup** (at session start, when making code changes):
- Create a feedback file at `.claude/feedback/feedback-{YYYY-MM-DD}-{short-id}.md` where `short-id` is a 6-character identifier (e.g., from the plan name or timestamp)
- The `.claude/feedback/` directory is gitignored — these files are for human review only

**What to log** — append entries whenever encountering something noteworthy:
- `[issue]` — bugs, unexpected behavior, workarounds applied
- `[improvement]` — code quality, performance, architecture ideas
- `[debt]` — things that work but could be better
- `[docs]` — missing or outdated documentation discovered

**Format**:
```markdown
## {date}

- [{category}] `{file_path}:{line}` — Brief description of the observation
- [{category}] `{file_path}` — Another observation
```

**Rules**:
- Append continuously as you work — do not wait for context compaction
- When delegating to subagents, pass the feedback file path in the prompt and instruct them to append their observations
- This is best-effort — do not let feedback capture slow down actual work
- One feedback file per conversation session

## Dependencies Policy

- Keep dependencies at their latest versions
- Rust deps: update in `Cargo.toml`, run `cargo update`
- JS deps: `pnpm up -r -i -L` to interactively update all
- `workspace:^` deps are rewritten by `pnpm publish` automatically

## Commit Convention

This project uses **conventional commits** for automatic changelog generation via [git-cliff](https://git-cliff.org/).

```
<type>(<scope>): <description>

Types:
  feat     - New feature
  fix      - Bug fix
  perf     - Performance improvement
  refactor - Code refactoring (no behavior change)
  docs     - Documentation only
  test     - Adding/updating tests
  chore    - Build, CI, tooling changes
  release  - Version bump and release

Scopes:
  core     - verter_core Rust crate
  napi     - verter_napi / @verter/native
  wasm     - verter_wasm / @verter/wasm
  play     - playground
  unplugin - @verter/unplugin
  lsp      - language-server
  types    - @verter/types
  ts       - @verter/core (TypeScript)
  meta     - @verter/component-meta
  ci       - CI/CD workflows
  *        - multiple areas

Examples:
  feat(core): add v-memo directive support
  fix(wasm): correct memory leak in compile()
  chore(ci): add nightly WASM build workflow
  release(all): v0.0.1-alpha.1
```

## CI/CD

See [docs/contributing/ci-cd.md](docs/contributing/ci-cd.md) for detailed CI/CD documentation including:

- Workflow specifications (CI, nightly, release)
- Pre-release versioning flow (alpha → beta → rc → stable)
- Publishing process (npm + crates.io)
- Nightly WASM builds and playground deployment
- Required GitHub secrets configuration

## Skills Reference

Detailed reference material is available as on-demand skills (loaded automatically when relevant):

| Skill | Use When |
|-------|----------|
| `/architecture` | Working on any specific module, need key files, type tables, LSP features, plugin system, analysis types |
| `/position-encoding` | Working with spans, positions, coordinate conversions, path normalization details |
| `/build-and-profiling` | Debugging build order, rebuild sequences, profiling, MCP server setup |
| `/testing` | Writing tests, test patterns, sourcemap testing, E2E workflow, server cleanup |
| `/wsl-e2e-testing` | Running E2E tests in WSL to reproduce Linux/CI failures, fixture matrix, acceptance criteria |
| `/rust-performance` | Optimizing Rust code, allocation patterns, batch operations, CodeTransform API |
