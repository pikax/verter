# Verter

Verter is a Vue compiler and Language Server Protocol (LSP) implementation. It converts Vue Single File Components (SFCs) to valid TSX (leveraging TypeScript for type checking) and compiles templates to optimized render functions. Unlike Volar, Verter generates actual valid TSX code rather than virtual files.

The project is a hybrid Rust + TypeScript monorepo: Rust crates handle template compilation (exposed via NAPI-RS native bindings and wasm-bindgen WASM) and the LSP server (`verter_lsp` binary, communicates over stdio), while TypeScript packages handle the SFC-to-TSX transformation and IDE integration.

## Architecture

### Package Dependency Graph

```
verter-vscode (VS Code extension)
├── verter-lsp (Rust LSP binary, stdio)
│   ├── verter_host (file host + compilation)
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
```

### Repository Structure

```
crates/
  verter_core/       # Core template compiler (Rust)
  verter_analysis/   # Static analysis: imports, exports, bindings, type resolution
  verter_host/       # In-memory file host: caching, dependency tracking, multi-file compilation
  verter_diagnostics/ # Vue SFC diagnostic engine: ~163 lint rules, rule trait, visitor, DiagnosticSet (depends only on verter_analysis)
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
  playground/        # @verter/playground - Online playground (private, Netlify-hosted)
  vue-vscode/        # verter-vscode - VS Code extension
  example/           # Example project
scripts/
  check-versions.mjs # Version check + publish order for CI
```

### Two Template Codegen Paths (CRITICAL)

The Rust compiler has **two separate template codegen paths**. Modifying one does NOT affect the other:

| Path | Module | Purpose | Output |
|------|--------|---------|--------|
| **VDOM/Vapor** | `template/code_gen/vdom/` | Runtime render functions for bundler output | `_createElementVNode(...)` calls |
| **IDE** | `ide/template/` | Valid JSX/TSX for LSP/TSGO type checking | `<div prop={expr}>` JSX elements |

The **LSP uses the IDE path** via `host.ensure_compiled()` with `CompileTarget::IDE`. TSGO type-checks this output. Changes to VDOM codegen do NOT affect LSP hover/completions. The IDE codegen auto-detects the script language: TS SFCs produce `.tsx` (TypeScript + JSX), while JS SFCs (no `lang` or `lang="js"`) produce `.jsx` (JavaScript + JSDoc annotations).

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

**TSGO known limitation**: Re-exported `.vue` components (e.g. barrel files like `export { default as MyComp } from './MyComp.vue'`) lose their typing when imported in another SFC. This is why `auto` mode defaults to tsserver. A warning is shown when TSGO is active. Remove the warning once this is resolved.

Only one provider runs at a time. Both use the `TypeProvider` trait (`tsgo/traits.rs`) with 14+ methods (hover, completions, diagnostics, definition, references, rename, etc.). Both are wrapped in a `ResilientTypeProvider` that detects crashes, auto-restarts (max 3 with exponential backoff), and replays the file cache.

**Key modules** (`crates/verter_lsp/src/`):
- `tsgo/` — TSGO integration (LSP client, resilient wrapper, project sync)
- `tsserver/mod.rs` — `find_tsserver()`, `find_node()`, `detect_ts_major_version()`
- `tsserver/ipc.rs` — `TsserverTypeProvider`, newline-delimited JSON transport, position conversion
- `tsserver/resilient.rs` — `ResilientTsserverProvider` (crash detection + auto-restart)
- `workspace_scanner.rs` — Async background workspace scanner with priority-based file loading

**Background file sync**: During `initialized()`, the LSP spawns a `WorkspaceScanner` background task that compiles ALL workspace `.vue` files to TSX and syncs them to the type provider asynchronously. This ensures imports of non-open `.vue` files resolve to actual component types rather than the wildcard `declare module '*.vue'` fallback.

**Freeze prevention** (fast typing): Three layers prevent tokio runtime starvation during rapid typing:
1. **SyncCoordinator** (`sync_coordinator.rs`): Single long-lived task replaces spawn-per-keystroke debounce. Uses mpsc channel + 300ms deadline map to guarantee exactly one sync per file after typing stops. Holds shared `Arc<VerterHost>`, `ProjectSync`, and `Arc<RwLock<PositionEncodingKind>>` (negotiated encoding from `initialize()`).
2. **Push diagnostics skip**: `did_change()` checks `is_typing_cooldown()` — when true, both `compute_verter_diagnostics` and `publish_diagnostics` are skipped. Pull diagnostics serve cached results. The SyncCoordinator publishes fresh diagnostics after typing stops.
3. **Hang detection** (`tsgo/ipc.rs`): `LspTransport` tracks `consecutive_failures` (AtomicU32). After 3 consecutive request timeouts, fires `crash_notify` to trigger `ResilientTypeProvider`'s existing restart machinery. Notifications use `try_send()` (non-blocking) to prevent channel backpressure.

**Heartbeat watchdog**: The server sends `$/verter/heartbeat` every 5s from `initialized()`. The VS Code extension monitors heartbeats — if none arrive for 30s, it auto-restarts the server. This is the last-resort safety net for runtime starvation.

**Async workspace scanning**: During `initialized()`, the LSP spawns a `WorkspaceScanner` background task instead of scanning synchronously. The scanner walks the filesystem, compiles `.vue` files to TSX, and syncs them to the type provider in priority order:

1. **Tier 0**: Files opened in the editor (signaled by `did_open`)
2. **Tier 1**: Project source files covered by `tsconfig.json` — siblings of open files first, then expanding outward
3. **Tier 2**: Remaining `.vue` files not covered by any tsconfig

TSGO sync is throttled (yield every 10 files) to prevent flooding. The scanner receives priority signals from `did_open` to dynamically re-order its queue. This makes `initialized()` return in <1s instead of blocking for the full scan duration.

**Key module**: `crates/verter_lsp/src/workspace_scanner.rs` — `WorkspaceScannerHandle`, `spawn_workspace_scanner()`, priority sorting, throttled sync loop.

### Multi-Root Workspace & Per-Project Configuration

In monorepo / multi-root VS Code workspaces, different packages have different `tsconfig.json` paths aliases, `.verterrc.json` lint rules, and `vite.config` resolve aliases. The LSP stores all workspace folders (`workspace_roots: Mutex<Vec<String>>`) and builds a `ProjectRegistry` that groups per-project configuration.

**Key types** (`crates/verter_lsp/src/config.rs`):
- `ProjectConfig` — per-project: root path, `TsConfigPathResolver`, `ResolvedLintConfig`, `Linter` instance
- `ProjectRegistry` — sorted by root length (longest prefix first), provides `find_project()`, `resolve_alias()`, `find_project_root()`, `linter_for()`

**Path alias resolution**: Each project's `TsConfigPathResolver` is built from its `tsconfig.json`. When `verter.viteConfig.enabled` is true (default), `discover_vite_aliases()` spawns Node.js to evaluate `vite.config.{ts,js,mjs}` and merges `resolve.alias` entries (vite aliases take precedence).

**Type provider integration**: TSGO receives `workspace/didChangeWorkspaceFolders` notifications. tsserver uses per-file `projectRootPath` from the project registry. Both resilient wrappers store workspace folders for restart replay.

**Lock ordering** (prevents deadlocks): `workspace_roots` (async) → `project_registry` (sync read) → release → `fallback_linter` (sync read). Never acquire `fallback_linter` while holding `project_registry`.

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
| `/rust-performance` | Optimizing Rust code, allocation patterns, batch operations, CodeTransform API |
