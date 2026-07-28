# LSP / MCP decoupling

`verter_lsp` does not depend on `verter_mcp`.

`verter_lsp` and `verter_mcp` are two independent products on top of the same
`verter_session` core. `cargo build -p verter_lsp` produces an LSP binary that
does not link `verter_mcp`, `rmcp`, or `axum`, and there is no Cargo feature
that re-embeds the MCP server into the LSP process. The `--mcp-port` CLI flag
is still parsed (so existing IDE configurations remain syntactically valid)
but the LSP logs a guidance warning and does not start an MCP server; the
`$/verter/mcpReady` notification is never sent.

## Why we decoupled

Tier-C cleanup objective: keep the dependency direction of the workspace
intentional. Coupling the two surfaces in `Cargo.toml` blurred that boundary,
increased the LSP's compile cost, made it impossible to ship an LSP build
that did not also build the MCP server, and let an MCP crash take the LSP
with it.

The architecture guard `lsp_mcp_dependency_direction`
(`crates/verter_session/tests/cases/architecture_guards.rs`) enforces that
`crates/verter_lsp/Cargo.toml` never declares `verter_mcp` as a non-optional
dependency.

## Running the MCP server

Spawn the standalone MCP binary:

```bash
cargo build -p verter_mcp
./target/debug/verter-mcp --project-root /path/to/project
```

This is the binary the release ships, both as the `verter-mcp` npm package and
as a `verter-mcp-<platform>` GitHub Release asset. It runs MCP in its own OS
process and shares no in-process state with `verter-lsp`. Clients pick whichever
transport they prefer (stdio for local agents, HTTP for remote agents) and route
notifications/requests directly to that process.

`crates/verter_mcp_server/` builds a second, behaviorally identical entry point
(`verter-mcp-server`); both binaries run the shared `verter_mcp::run::run`
body. It exists so no crate ever needs a dependency edge to `verter_mcp` just
to name an entry point. It is not distributed; use `verter-mcp`.

### HTTP readiness record

With `--transport http`, the server binds before its initial project scan and
announces the bound port (OS-assigned under `--port 0`) as exactly one JSON
line on stdout:

```json
{"verterMcpHttpReady":{"port":54321,"url":"http://127.0.0.1:54321/mcp"}}
```

Human `tracing` output goes to stderr and is not port identity. A spawning
host must parse this record — the encoding contract lives in
`crates/verter_mcp/src/readiness.rs`, with the TypeScript mirror parser in
`packages/vue-vscode/src/mcpServer.ts`.

## What the VS Code extension does

When `verter.mcp.enabled` (default true), the extension spawns the standalone
`verter-mcp` binary with `--transport http --port <verter.mcp.port>` (default
0 = auto-assign) and `--client-pid <extension host pid>`, parses the readiness
record, registers the endpoint with VS Code's MCP provider API
(`lm.registerMcpServerDefinitionProvider`), and mirrors the port into the
workspace's `.mcp.json` for Claude Code CLI.

`--client-pid` is the same containment contract as `verter-lsp`: the server
exits when the named host process dies, so a hard extension-host kill cannot
orphan an HTTP listener (`verter_tsgo_api::process::ClientProcessGuard`).
The extension-side supervisor (`createMcpServerLifecycle`) replaces a child
on config change only after the predecessor has really exited, and answers a
post-ready crash by removing the provider registration and respawning within
a bounded budget.

Binary discovery reuses the `verter-mcp` npm launcher (installed platform
package → workspace `target/{debug,release}` dev build → the VSIX-staged
`bin/verter-mcp` → `PATH`). Release packaging fails closed: `package.mjs`
inspects the packed VSIX and refuses to produce one without the per-target
`bin/verter-mcp` engine.

The extension still passes `--mcp-port` to the LSP for now: the flag is inert
server-side (guidance warning only), and the DX log canary
(`packages/vue-vscode/e2e/dx/dxLogCanary.ts`) deliberately uses that
deterministic warning as its capture probe. Removing the flag from
`buildLspLaunchArgs` is gated on the canary adopting a replacement trigger.
