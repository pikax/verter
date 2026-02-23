# Hotpath MCP Setup

This folder contains the MCP config used to expose hotpath profiling data to AI agents.

## 1) Start the MCP server

From repo root, run one of the following:

```bash
# AST-only pipeline (tokenize + parse + OXC expressions):
pnpm run profile:hotpath:mcp

# Full compile pipeline (tokenize + parse + style + script + template codegen):
pnpm run profile:hotpath:full:mcp
```

This starts profiling and serves MCP on:

- `http://localhost:6771/mcp`

Keep this process running while the agent is connected.

## 2) Use the checked-in MCP config

Use this config file directly:

- `mcp/hotpath.mcp.json`

Contents:

```json
{
  "mcpServers": {
    "verter-hotpath": {
      "url": "http://localhost:6771/mcp"
    }
  }
}
```

## 3) Agent/client wiring

Most MCP-capable tools accept this same `mcpServers` JSON shape. If your client does not support loading a file directly, copy the server entry into its local MCP config.

### Copilot CLI / Copilot agent runtimes

- Point the tool to `mcp/hotpath.mcp.json` when it supports a custom MCP config path.
- In CI-style Copilot flows, this is commonly provided via an environment variable (for example `GH_AW_MCP_CONFIG`).

### Claude-based MCP clients

- Add a server named `verter-hotpath` with URL `http://localhost:6771/mcp` in your MCP config.
- If your client supports importing from file, import `mcp/hotpath.mcp.json`.

### Cursor / editor MCP UIs

- Add an MCP server with name `verter-hotpath` and URL `http://localhost:6771/mcp`.
- If a JSON config option exists, paste the `mcpServers` snippet above.

## 4) Quick verification

If your agent can list MCP tools from `verter-hotpath`, setup is complete. A plain browser/fetch request may return HTTP `Not Acceptable`, which is normal for MCP endpoints that expect MCP protocol messages.
