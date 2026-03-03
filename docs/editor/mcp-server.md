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

| Setting | Default | Description |
|---------|---------|-------------|
| `verter.mcp.enabled` | `true` | Start the MCP HTTP endpoint alongside the LSP |
| `verter.mcp.port` | `6772` | Port for the MCP HTTP endpoint (0 for auto-assign) |
| `verter.mcp.lintPreset` | `"recommended"` | Lint preset for MCP diagnostic tools |
| `verter.mcp.claudeCodeNotification` | `true` | Show notification when Claude Code is detected |

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

| Command | ID |
|---------|----|
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

The MCP server exposes 36+ tools organized by category:

### Project Management
- `scan_project` — Load all `.vue` files from a directory
- `upsert_file` — Add or update a single file

### Analysis
- `analyze_file` — Full SFC analysis (imports, exports, bindings, macros)
- `get_template_analysis` — Template-level analysis (elements, directives, slots)
- `get_component_api` — Props, emits, slots, and exposed members
- `get_binding_types` — Reactivity classification for all bindings

### Diagnostics
- `lint_file` — Run lint rules on a file
- `lint_project` — Lint all loaded files
- `get_fix_suggestions` — Get auto-fix suggestions for diagnostics

### Compilation
- `compile_tsx` — Compile SFC to TSX (IDE output)
- `compile_vdom` — Compile template to VDOM render function
- `compile_vapor` — Compile template to Vapor render function

### Cross-File Intelligence
- `find_component_usage` — Find where a component is used across the project
- `get_import_graph` — Dependency graph for loaded files
- `get_type_deps` — Cross-file type dependencies for a component

### Scoring & Quality
- `score_file` — Quality score for a single file
- `score_project` — Aggregate quality scores across the project

## Recommended Workflow

For AI agents using the MCP server:

1. **Start with `scan_project`** to load the workspace
2. **Use `analyze_file`** to understand a component before modifying it
3. **Use `lint_file`** after changes to catch issues
4. **Use `find_component_usage`** before renaming or changing component APIs
