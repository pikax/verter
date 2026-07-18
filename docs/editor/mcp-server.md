# MCP Server

Verter includes a built-in [Model Context Protocol (MCP)](https://modelcontextprotocol.io/) server that exposes Vue analysis tools to AI agents like Claude Code and GitHub Copilot.

## How It Works

The MCP server runs as an HTTP endpoint embedded directly in the LSP process. It shares the same `VerterHost` instance, so AI agents get access to:

- All parsed and analyzed `.vue` files (no duplicate loading)
- Full cross-file type resolution via TSGO
- Diagnostic results and lint rules
- Code actions and quick fixes

```
verter-lsp process
├── LSP (stdio) ← VS Code
└── MCP (HTTP :6772) ← Claude Code / AI agents
```

## Enabling

The MCP server is **enabled by default**. When the VS Code extension starts the LSP, it automatically starts the MCP HTTP endpoint on port `6772`.

### Settings

| Setting                             | Default         | Description                                        |
| ----------------------------------- | --------------- | -------------------------------------------------- |
| `verter.mcp.enabled`                | `true`          | Start the MCP HTTP endpoint alongside the LSP      |
| `verter.mcp.port`                   | `6772`          | Port for the MCP HTTP endpoint (0 for auto-assign) |
| `verter.mcp.lintPreset`             | `"recommended"` | Lint preset for MCP diagnostic tools               |
| `verter.mcp.claudeCodeNotification` | `true`          | Show notification when Claude Code is detected     |

To disable:

```json
{
  "verter.mcp.enabled": false
}
```

## Claude Code Setup

### Automatic Detection

When the Verter extension detects Claude Code is installed (`~/.claude/` directory exists), it shows a notification offering to configure MCP automatically.

Clicking **"Setup Now"** creates or updates `.mcp.json` in your workspace root:

```json
{
  "mcpServers": {
    "verter": {
      "url": "http://localhost:6772/mcp"
    }
  }
}
```

### Manual Setup

Run the command **"Verter: Setup MCP for Claude Code"** from the command palette (`Ctrl+Shift+P`), or manually create `.mcp.json` in your workspace root with the config above.

After configuring, restart Claude Code to activate the MCP connection.

### Command

| Command                   | ID                             |
| ------------------------- | ------------------------------ |
| Setup MCP for Claude Code | `verter.setupMcpForClaudeCode` |

## Standalone Usage

The standalone `verter-mcp` binary is also available for use without VS Code:

```bash
# stdio transport (for local agents)
verter-mcp --project-root /path/to/project

# HTTP transport (for remote agents)
verter-mcp --transport http --port 6772 --project-root /path/to/project
```

The standalone binary creates its own `VerterHost` and does not share data with the LSP. For the best experience, use the embedded MCP server via the VS Code extension.

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
