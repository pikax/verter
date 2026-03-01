# Verter MCP Servers

Verter exposes two MCP (Model Context Protocol) servers for AI agents:

| Server | Purpose | Transport | Port |
|--------|---------|-----------|------|
| **verter-mcp** | Analysis, diagnostics, compilation, scoring | stdio or HTTP | 6772 |
| **verter-hotpath** | Hotpath profiling (timing + allocation) | HTTP | 6771 |

---

## Analysis & Diagnostics MCP (`verter-mcp`)

A standalone binary exposing Verter's full Vue SFC analysis pipeline to AI agents. Provides 33 tools across file management, static analysis, diagnostics, compilation, cross-file intelligence, scoring, and refactoring.

### Quick Start

```bash
# Build
cargo build -p verter_mcp --release

# Run (stdio — for agent child process)
verter-mcp --project-root .

# Run (HTTP — for remote/shared access)
verter-mcp --transport http --project-root .
# Serves at http://localhost:6772/mcp
```

### CLI Options

```
verter-mcp [OPTIONS]

Options:
  --transport <stdio|http>     Transport mode (default: stdio)
  --port <PORT>                HTTP port (default: 6772)
  --project-root <PATH>        Project root for initial scan
  --lint-preset <PRESET>       Lint preset: essential|recommended|all|strict|a11y|performance
  --no-scan                    Skip initial directory scan
```

### MCP Config Files

**stdio** (`mcp/verter.mcp.json`):
```json
{
  "mcpServers": {
    "verter": {
      "command": "verter-mcp",
      "args": ["--project-root", "."]
    }
  }
}
```

**HTTP** (`mcp/verter-http.mcp.json`):
```json
{
  "mcpServers": {
    "verter": {
      "url": "http://localhost:6772/mcp"
    }
  }
}
```

### Tools (33 total)

#### Agent-First Summary Tools (start here)

| Tool | Description |
|------|-------------|
| `get_component_summary` | Everything about a component: API, scores, metrics, diagnostics, CSS, deps |
| `get_project_stats` | Aggregate project stats: scores, Vue API usage, diagnostics health, rankings |
| `get_component_quality` | Quality score (0-100) with per-dimension breakdown |

#### File Management

| Tool | Description |
|------|-------------|
| `scan_project` | Scan directory, load all `.vue` files into host |
| `upsert_file` | Load or update a single file (reads from disk or accepts source) |
| `list_files` | List all loaded files |

#### Deep Analysis

| Tool | Description |
|------|-------------|
| `analyze_file` | Full analysis snapshot (script, template, styles sections) |
| `get_component_api` | Props, emits, slots, models, expose, inheritsAttrs |
| `get_imports` | Imports with Vue API classification |
| `get_bindings` | Bindings with ReactivityKind (Ref/Computed/Reactive/etc.) |
| `get_template_usage` | Components, binding refs, slots, template refs, event handlers |

#### CSS

| Tool | Description |
|------|-------------|
| `analyze_css` | Style blocks with selectors, classes, IDs, custom props, v-bind |
| `match_css_selector` | Per-element match results (Matches/MaybeMatches/NoMatch) |
| `detect_css_bleed` | Unintended cross-component CSS class bleed detection |

#### Diagnostics

| Tool | Description |
|------|-------------|
| `lint_file` | Lint a single file with configurable preset |
| `lint_project` | Lint all loaded files with summary |
| `get_quick_fixes` | Code actions available at a given offset |

#### Compilation

| Tool | Description |
|------|-------------|
| `compile_file` | Compile to JS/CSS (production, vapor, source maps) |
| `generate_tsx` | Generate TSX output (type-checking path) |

#### Cross-File Intelligence

| Tool | Description |
|------|-------------|
| `get_component_graph` | Component dependency graph (BFS traversal) |
| `validate_provide_inject` | Missing providers, unused provides across project |
| `check_component_props` | Unknown props/models passed to child components |
| `find_orphan_components` | Components unreachable from entry points |

#### Runtime Behavior Hints

| Tool | Description |
|------|-------------|
| `get_lifecycle_order` | Lifecycle hooks in execution order with side effects flagged |
| `get_rerender_triggers` | Which reactive bindings cause which template regions to update |
| `get_side_effects` | All side effects: lifecycle hooks, watchers, provide/inject, DOM queries |

#### Refactoring Suggestions

| Tool | Description |
|------|-------------|
| `suggest_refactorings` | Auto-detected opportunities: extract component, simplify bindings |
| `detect_prop_drilling` | Props passed through 2+ levels — suggests provide/inject |
| `detect_migration_targets` | Options API → Composition API candidates with difficulty estimate |

#### Type System

| Tool | Description |
|------|-------------|
| `get_component_types` | Inferred/declared types for props, emits, slots, bindings |
| `check_prop_types` | Type compatibility between parent prop values and child declarations |
| `get_type_errors` | Type-level diagnostics from TSX codegen path |

#### Documentation & Utility

| Tool | Description |
|------|-------------|
| `generate_component_docs` | Auto-generated Markdown docs (props table, events, slots, usage) |
| `explain_vue_api` | Explain any Vue Composition API function |

### Recommended Agent Workflow

1. **Start with `get_project_stats`** — understand the project at a glance
2. **Use `get_component_summary`** for any component you're working with — it returns everything
3. **Drill into specifics** with `analyze_file`, `get_component_api`, `lint_file` as needed
4. **Cross-file checks** with `get_component_graph`, `validate_provide_inject`, `find_orphan_components`
5. **Refactoring** with `suggest_refactorings`, `detect_prop_drilling`

---

## Hotpath Profiling MCP (`verter-hotpath`)

Exposes hotpath profiling data (timing and allocation metrics) for performance analysis.

### Quick Start

```bash
# AST-only pipeline (tokenize + parse + OXC expressions):
pnpm run profile:hotpath:mcp

# Full compile pipeline (tokenize + parse + style + script + template codegen):
pnpm run profile:hotpath:full:mcp
```

Serves MCP at `http://localhost:6771/mcp`. Keep the process running while the agent is connected.

### MCP Config

Use `mcp/hotpath.mcp.json`:

```json
{
  "mcpServers": {
    "verter-hotpath": {
      "url": "http://localhost:6771/mcp"
    }
  }
}
```

---

## Agent/Client Wiring

Most MCP-capable tools accept the `mcpServers` JSON shape. If your client doesn't support loading a file directly, copy the server entry into its local MCP config.

### Claude Code

```bash
# Add verter analysis MCP (stdio — recommended)
claude mcp add verter -- verter-mcp --project-root .

# Or use the config file
# Add the contents of mcp/verter.mcp.json to your MCP settings
```

### Cursor / VS Code MCP

Add an MCP server with the appropriate URL or command from the config files above.

### Copilot CLI / Copilot Agent

Point to the config file path, or set via `GH_AW_MCP_CONFIG` environment variable.

## Quick Verification

If your agent can list MCP tools from `verter` (33 tools) or `verter-hotpath`, setup is complete. A browser request to the HTTP endpoint may return `Not Acceptable` — this is normal for MCP protocol endpoints.
