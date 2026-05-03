# LSP / MCP decoupling

`verter_lsp` no longer depends on `verter_mcp` by default.

The MCP server is now optional and lives behind the `mcp` Cargo feature:

```toml
# crates/verter_lsp/Cargo.toml
[features]
mcp = ["dep:verter_mcp", "dep:rmcp", "dep:axum"]

[dependencies]
verter_mcp = { path = "../verter_mcp", optional = true }
rmcp       = { version = "1", features = [...], optional = true }
axum       = { version = "0.8", optional = true }
```

`cargo build -p verter_lsp` produces an LSP binary that does not link
`verter_mcp`, `rmcp`, or `axum`. The `--mcp-port` CLI flag is still
parsed (so existing IDE configurations remain syntactically valid) but
the LSP logs a warning and does not start an MCP HTTP server.

## Why we decoupled

Tier-C cleanup objective: keep the dependency direction of the workspace
intentional. `verter_lsp` and `verter_mcp` are two independent surfaces
on top of the same `verter_session` core. Coupling them in
`Cargo.toml` blurred that boundary, increased the LSP's compile cost,
and made it impossible to ship an LSP build that did not also build
the MCP server.

The architecture guard `lsp_mcp_dependency_direction`
(`crates/verter_session/tests/architecture_guards.rs`) enforces that
`verter_lsp/Cargo.toml` declares `verter_mcp` only as
`optional = true`.

## Migration paths for clients that previously consumed MCP-via-LSP

There are now two supported ways to run the MCP server:

### Option 1 — Spawn the standalone `verter-mcp-server` binary

```bash
cargo build -p verter_mcp_server
./target/debug/verter-mcp-server --project-root /path/to/project
```

`verter-mcp-server` is a thin binary in `crates/verter_mcp_server/`.
It runs MCP in its own OS process and shares no in-process state
with `verter-lsp`. Clients pick whichever transport they prefer
(stdio for local agents, HTTP for remote agents) and route
notifications/requests directly to that process.

This is the recommended path for IDEs that previously asked
`verter-lsp` to start MCP via `--mcp-port`. Spawning a dedicated
process keeps the LSP binary small and prevents an MCP crash from
taking the LSP with it.

### Option 2 — Build the LSP with `--features mcp`

```bash
cargo build -p verter_lsp --features mcp
./target/debug/verter-lsp --mcp-port=0
```

This restores the legacy behavior: a single `verter-lsp` process that
also runs an MCP HTTP server on the given port. Suitable for
distributions that want a single binary and accept the larger compile
graph and shared-process failure mode.

## What clients see

* `--mcp-port` on a feature-disabled build → LSP logs a warning and
  does not start MCP. The `$/verter/mcpReady` notification is not sent.
* `--mcp-port` on `--features mcp` → identical behavior to the
  pre-decoupling LSP: an HTTP MCP server is bound to
  `127.0.0.1:<port>` and `$/verter/mcpReady` is sent with the actual
  bound port.
* `verter-mcp-server` in its own process → identical CLI to the legacy
  `verter-mcp` binary in `crates/verter_mcp/`. The two binaries
  delegate to the same `VerterMcpServer` implementation.
