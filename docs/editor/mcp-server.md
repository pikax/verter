# MCP Server

Verter includes a built-in [Model Context Protocol (MCP)](https://modelcontextprotocol.io/) server that exposes Vue analysis tools to AI agents like Claude Code and GitHub Copilot.

## How It Works

The MCP server is a standalone binary, `verter-mcp`, that an agent launches in
its own process. It builds a `VerterHost` over the project root it is given, so
agents get:

- Parsing and analysis of every `.vue` and `.svelte` file under the project root
- Full cross-file type resolution
- Diagnostic results and lint rules
- Code actions and quick fixes

```
verter-mcp process
└── MCP (stdio by default, HTTP optional) ← Claude Code / AI agents
```

::: warning The LSP no longer hosts MCP in-process
Earlier versions ran the MCP server as an HTTP endpoint inside the `verter-lsp`
process, and the VS Code extension started it via `--mcp-port`. That embedding
was removed: `verter-lsp` parses `--mcp-port` only to warn that the standalone
binary owns MCP now.

In VS Code, the `verter.mcp.*` settings drive the standalone binary instead:
when `verter.mcp.enabled` (the default), the extension spawns `verter-mcp
--transport http` with an auto-assigned port, registers the endpoint with
VS Code's MCP provider API (so Copilot Chat discovers it), and updates the
port of an existing `verter` entry in the workspace's `.mcp.json` for Claude
Code CLI. Outside VS Code, launch `verter-mcp` from your agent's MCP config as
shown below.
:::

## Install

```bash
pnpm add -D verter-mcp
```

The `verter-mcp` package is a launcher: the native server ships in a
per-platform optional dependency (`@verter/mcp-<platform>`) and your package
manager installs only the one matching your OS, architecture and libc. Supported
platforms are macOS x64/arm64, Linux x64/arm64 (glibc and musl) and Windows x64.

Installing it as a project dev dependency pins the server version alongside the
rest of your Verter tooling. `npx verter-mcp --print-server-path` prints the
absolute path of the native binary for clients that want to spawn it directly.

## Running it

Stdio is the default transport, which is what local agents use:

```bash
npx verter-mcp --project-root .
```

For a remote agent, serve it over HTTP instead:

```bash
npx verter-mcp --transport http --port 6772 --project-root .
```

## Claude Code Setup

Add `verter-mcp` to `.mcp.json` in your workspace root:

```json
{
  "mcpServers": {
    "verter": {
      "command": "npx",
      "args": ["-y", "verter-mcp", "--project-root", "."]
    }
  }
}
```

With `verter-mcp` installed as a project dev dependency you can point at the
local launcher instead, which avoids `npx` resolution entirely:

```json
{
  "mcpServers": {
    "verter": {
      "command": "./node_modules/.bin/verter-mcp",
      "args": ["--project-root", "."]
    }
  }
}
```

After configuring, restart Claude Code to activate the MCP connection.

::: tip The extension's setup command writes the live HTTP endpoint
The **"Verter: Setup MCP for Claude Code"** command
(`verter.setupMcpForClaudeCode`) writes the running standalone server's
actual `http://127.0.0.1:<port>/mcp` endpoint. When nothing is running yet
(for example, no `.vue`/`.svelte` file has been opened), the command starts
the server and waits for its bound port; if the server cannot become ready —
or `verter.mcp.enabled` is off — the command explains why and writes
nothing, rather than persisting a dead endpoint. The `command` forms above
remain the right choice for agents that should own the server process
themselves (stdio transport).
:::

## Without npm

If you would rather not install from npm, download the `verter-mcp-<platform>`
binary from a [GitHub release](https://github.com/pikax/verter/releases), or
build it from a checkout:

```bash
cargo build -p verter_mcp --bin verter-mcp --release
```

The binary creates its own `VerterHost` over `--project-root` and does not share
state with a running LSP process.

## Available Tools

The MCP server exposes **49 tools** organized by category. A representative selection is shown below; see [mcp/README.md](https://github.com/pikax/verter/tree/main/mcp) for the complete reference.

### File Management

- `scan_project` — Load all `.vue` files from a directory
- `upsert_file` — Add or update a single file
- `list_files` — List all loaded files

### Analysis

- `analyze_file` — Full SFC analysis (imports, exports, bindings, macros)
- `get_component_api` — Props, emits, slots, models, and exposed members
- `get_bindings` — Reactivity classification for all bindings
- `get_imports` — Imports with Vue API classification
- `get_template_usage` — Components, binding refs, slots, template refs, event handlers
- `get_framework_surface` — Resolve props/emits/slots/expose through the framework-surface executor

### Diagnostics

- `lint_file` — Run lint rules on a file
- `lint_project` — Lint all loaded files
- `get_quick_fixes` — Code actions available at a given offset

### Compilation

- `compile_file` — Compile to JS/CSS (production, Vapor, source maps)
- `generate_tsx` — Generate TSX output (IDE type-checking path)

### CSS

- `analyze_css` — Style blocks with selectors, classes, custom props, `v-bind`
- `match_css_selector` — Per-element selector match results
- `detect_css_bleed` — Cross-component CSS class bleed detection

### Cross-File Intelligence

- `get_component_graph` — Component dependency graph
- `validate_provide_inject` — Missing providers / unused provides across the project
- `check_component_props` — Unknown props passed to child components
- `find_orphan_components` — Components unreachable from entry points

### Type System

- `get_component_types` — Inferred/declared types for props, emits, slots, bindings
- `check_prop_types` — Type compatibility between parent values and child declarations
- `get_type_errors` — Type-level diagnostics from the TSX codegen path

### Scoring & Summaries

- `get_component_summary` — Everything about a component in one call
- `get_component_quality` — Quality score (0–100) with per-dimension breakdown
- `get_project_stats` — Aggregate project scores, Vue API usage, and diagnostics health

### Routing, Stores & SSR

- `get_route_tree`, `get_route_for_component`, `analyze_route_health` — vue-router / Nuxt route analysis
- `get_store_graph`, `trace_store_flow` — store (Pinia) dependency analysis
- `ssr_readiness`, `ssr_migration_plan`, `ssr_project_report` — SSR safety scoring

## Recommended Workflow

For AI agents using the MCP server:

1. **Start with `scan_project`** to load the workspace
2. **Use `analyze_file`** to understand a component before modifying it
3. **Use `lint_file`** after changes to catch issues
4. **Use `get_component_graph`** before renaming or changing component APIs
