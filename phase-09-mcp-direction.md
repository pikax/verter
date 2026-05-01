# Phase 9.2 — MCP↔LSP dependency direction inventory

This is the deferred Phase 9.2 informational inventory called out in
the cutover plan §9.2 (lines 6415-6436 of
`D:/tmp/verter-architecture-cutover.md`). It enumerates every symbol
that crosses the `verter_lsp` ↔ `verter_mcp` crate boundary so a
follow-up sub-plan can decide whether the direction is sound.

Phase 9.2 is **informational only** — no extraction work is performed
here. Per §9.2: "Worker DOES NOT IMPLEMENT extraction — that is a
sub-plan."

## Scope

Two crates:

- `crates/verter_lsp/` — the LSP server crate (binary `verter_lsp`).
- `crates/verter_mcp/` — the MCP server crate (binary `verter-mcp`,
  also has a `lib` target consumed by `verter_lsp`).

The §9.2 grep template (pre-Phase-11e: `crates/verter_lsp/src/server.rs`)
is adapted to the post-Phase-11e tree where `server.rs` was split into
the `server/` folder module rooted at `server/mod.rs`.

## Greps performed (current tree, post-Phase-11)

LSP → MCP edges:

```
grep -n "verter_mcp\|use verter_mcp" \
    crates/verter_lsp/Cargo.toml \
    crates/verter_lsp/src/lib.rs \
    crates/verter_lsp/src/server/mod.rs
```

The `lib.rs` and `server/mod.rs` greps both returned no matches. The
MCP wiring lives entirely in the binary entrypoint (`main.rs`), which
is consistent with the binary-only consumer pattern (the LSP library
itself does NOT depend on `verter_mcp`; only the LSP **binary** does,
to host the MCP transport in-process).

A workspace-wide grep was also run to confirm no other `verter_lsp/src/`
file references `verter_mcp`:

```
Grep pattern: "verter_mcp"
Path:         crates/verter_lsp/src
Result:       2 matches, both in main.rs (lines 464, 466)
```

MCP → LSP edges:

```
grep -n "verter_lsp\|use verter_lsp" \
    crates/verter_mcp/Cargo.toml \
    crates/verter_mcp/src/lib.rs
```

Result: zero matches. `verter_mcp` does NOT depend on `verter_lsp`.

## Cross-crate import inventory

### `crates/verter_lsp/Cargo.toml`

| Line | Edge | Symbol | Notes |
| ---- | ---- | ------ | ----- |
| 25   | LSP→MCP (Cargo dependency) | `verter_mcp = { path = "../verter_mcp" }` | Plain workspace path dep, no feature flags. |

### `crates/verter_lsp/src/main.rs`

| Line | Edge | Symbol | Notes |
| ---- | ---- | ------ | ----- |
| 464  | LSP→MCP (`use`) | `verter_mcp::McpServerConfig` | Imported at the top of `serve_mcp_http()` only. |
| 464  | LSP→MCP (`use`) | `verter_mcp::VerterMcpServer` | Imported at the top of `serve_mcp_http()` only. |
| 466  | LSP→MCP (path call) | `verter_mcp::tools::diagnostics::make_lint_config` | Called once per HTTP server startup to build the lint config from the LSP-side `--lint-preset` flag. |

`serve_mcp_http()` constructs a `VerterMcpServer` over the
`StreamableHttpService` rmcp transport and serves it on a TCP listener
that `main()` already bound. The shared `Arc<VerterHost>` is passed
across the boundary (LSP-owned host, served by the MCP transport).

### `crates/verter_mcp/Cargo.toml`

No `verter_lsp` dependency. Direct deps in this crate:
`verter_session`, `verter_semantic`, `verter_workspace`,
`verter_diagnostics`, `verter_actions`. None of those re-export
`verter_lsp` symbols.

### `crates/verter_mcp/src/lib.rs`

No `verter_lsp` reference.

## Cross-crate boundary summary

| Direction       | Edge count | Surface                                  |
| --------------- | ---------- | ---------------------------------------- |
| LSP → MCP       | 1 (Cargo)  | `Cargo.toml:25` — workspace path dep.    |
| LSP → MCP       | 3 (use/path) | `main.rs:464,466` — three symbols (`McpServerConfig`, `VerterMcpServer`, `tools::diagnostics::make_lint_config`). |
| MCP → LSP       | 0          | None.                                    |

## Direction analysis

The direction is **strictly LSP → MCP** (one-way). Specifically:

1. The LSP **library** (`crates/verter_lsp/src/lib.rs`,
   `server/mod.rs`, `background_init.rs`, ...) does NOT depend on
   `verter_mcp` at all.
2. The LSP **binary** (`crates/verter_lsp/src/main.rs`) depends on
   `verter_mcp` only inside the `serve_mcp_http()` function — the
   in-process MCP host that runs alongside the LSP server when the
   user opts in via the `--mcp-port` flag.
3. The MCP crate does NOT depend on `verter_lsp` at all (no
   back-edge).

## Symbols crossing the boundary

Three public `verter_mcp` symbols are reached from `verter_lsp`:

1. `verter_mcp::McpServerConfig` — config struct (constructed via
   `Default::default()` at `main.rs:468`).
2. `verter_mcp::VerterMcpServer` — server struct (constructed via
   `VerterMcpServer::new(host, linter, config)` at `main.rs:469`).
3. `verter_mcp::tools::diagnostics::make_lint_config` — public free
   function (called at `main.rs:466`).

The shared runtime types crossing the boundary by value are:

- `Arc<VerterHost>` (owned by the LSP, passed by reference into the
  MCP server constructor).
- `Arc<verter_diagnostics::Linter>` (constructed in the LSP binary
  from `verter_mcp::tools::diagnostics::make_lint_config`'s output;
  the linter type itself comes from `verter_diagnostics`, not from
  `verter_mcp`).

## Architectural notes (informational)

The current direction is consistent with "LSP is the long-running
host, MCP is an optional in-process tool surface served beside it." It
also avoids the inverse direction (which would force `verter_mcp` to
pull in the LSP framework, which it does not need). The §9.2 brief
flags this inventory only — no architectural finding is surfaced.

If a future sub-plan extracts the in-process MCP transport hosting
out of `verter_lsp/src/main.rs`, the three `use` lines and the one
`Cargo.toml` dep are the entire LSP-side surface to migrate. No deeper
coupling exists.

## Closes

Phase 9.2 deferred item per cutover state.
