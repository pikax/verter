---
name: build-and-profiling
description: "Build dependency chains, rebuild sequences, profiling with MCP, and Analysis MCP server setup for Verter"
---

# Build Dependency Chain & Profiling

## Build Dependency Chain

When changing Rust code, you must rebuild downstream artifacts in order:

```
verter_core + verter_analysis + verter_host + verter_ffi (Rust crates)
    ↓ cargo build
verter_napi (NAPI-RS cdylib)    verter_lsp (LSP binary)    verter_wasm (wasm-bindgen cdylib)
    ↓ pnpm run build:native         ↓ pnpm run build:lsp       ↓ pnpm run build:wasm
@verter/native (.node binary)   verter-lsp (target/debug/)  @verter/wasm (WASM pkg)
    ↓                                ↓                          ↓
@verter/unplugin (bundler)      verter-vscode (F5/VSIX)     @verter/playground (browser)
    ↓
playground build (Vite)
    ↓
playground E2E tests
```

## Common Rebuild Sequences

| What changed | Rebuild commands (in order) |
|---|---|
| Rust crate (`verter_core`) | `pnpm run build:native` → rebuild any downstream consumer |
| Rust LSP (`verter_lsp`) | `pnpm run build:lsp` (or `build:lsp:release` for optimized) → restart VS Code extension host |
| Unplugin (`packages/unplugin`) | `pnpm run build:ts` (or just rebuild unplugin) |
| Playground after Rust/unplugin change | `pnpm run build:native` → `cd packages/playground && rm -rf dist node_modules/.vite && npx vite build` |
| WASM (for playground browser editor) | `pnpm run build:wasm` |
| Everything | `pnpm build` (runs native → lsp → wasm → ts in correct order) |

## Key Details

- `@verter/unplugin` depends on `@verter/native` — compiles `.vue` files at build time via the Rust native binary
- `@verter/playground` uses `@verter/unplugin` (devDep) for its own Vue SFC compilation, and `@verter/wasm` (dep) for the in-browser editor
- The native binary lives in `packages/native/dist/` after `build:native`
- The LSP binary lives in `target/debug/verter-lsp` (or `target/release/verter-lsp` with `build:lsp:release`)
- Clear Vite cache (`node_modules/.vite`) when rebuilding playground after native changes

## Quick Rebuild (Native)

```bash
# Quick rebuild native + copy
cargo build --release --package verter_napi && rm -f packages/native/dist/verter-native.win32-x64-msvc.node && cp target/release/verter_napi.dll packages/native/dist/verter-native.win32-x64-msvc.node
```

## Profiling with MCP

Use the real-world profiling example with hotpath instrumentation. Two pipeline modes are available:

```bash
# AST-only pipeline (tokenize → parse → OXC expressions):
pnpm run profile:hotpath          # Timing hotspots
pnpm run profile:hotpath:alloc    # Timing + allocation hotspots
pnpm run profile:hotpath:mcp      # Starts MCP endpoint at http://localhost:6771/mcp

# Full compile pipeline (tokenize → parse → style → script → template codegen):
pnpm run profile:hotpath:full          # Timing hotspots
pnpm run profile:hotpath:full:alloc    # Timing + allocation hotspots
pnpm run profile:hotpath:full:mcp      # Starts MCP endpoint at http://localhost:6771/mcp
```

The full pipeline exercises all instrumented functions across the compilation flow:
compile, generate_script, process_script_setup, process_macro_item, generate_style,
process_style, apply_scoped_normalized, parse_template_expressions, generate_template,
walk_template, apply_to, batch_overwrite, batch_prepend_left_static, build_string,
generate_map, generate_map_json, alloc_node, attach_to_parent.

Agent MCP config template is checked in at:

```text
mcp/hotpath.mcp.json
```

Point your MCP-capable agent to that file (or copy its `mcpServers` entry into your local MCP config).
For client-specific setup examples, see [mcp/README.md](mcp/README.md).

## Analysis MCP Server (`verter_mcp`)

The `verter-mcp` binary exposes Verter's full analysis, diagnostics, compilation, and scoring pipeline via MCP. It provides 33 tools for AI agents to deeply understand Vue codebases without reading files directly.

```bash
# Build
pnpm run build:mcp            # Debug build
pnpm run build:mcp:release    # Release build

# Run (stdio — agent spawns as child process)
verter-mcp --project-root /path/to/vue-project

# Run (HTTP — remote/shared access)
verter-mcp --transport http --project-root /path/to/vue-project
# Serves at http://localhost:6772/mcp
```

MCP config files are checked in at:
- `mcp/verter.mcp.json` (stdio)
- `mcp/verter-http.mcp.json` (HTTP)

For the full tool catalog and agent workflow guide, see [mcp/README.md](mcp/README.md).
