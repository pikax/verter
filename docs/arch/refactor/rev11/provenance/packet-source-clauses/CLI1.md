# Exact operative source-clause attachment — CLI1

Schema: 1. Node: `CLI1`. Clause count: 2. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-EXP-L1484-84EF6BE01E98

- Kind: `context`; source: `successor-expansion.md:1484-1484`; target: `node:CLI1`; text SHA-256: `84ef6be01e989164fdecbdd9b3ac8c5a269b004d3df7dae8d361b43c68144937`.

~~~~markdown
### `CLI1.md` — Shared application services, selection, invocation, reporters
~~~~

### SRC-EXP-L1486-1DD61FB91A5D

- Kind: `forbidden`; source: `successor-expansion.md:1486-1491`; target: `node:CLI1`; text SHA-256: `1dd61fb91a5da26a81ac6e8e6f3aeebcce9db786b8700d063f3ca48643a47ced`.

~~~~markdown
**Intent:** implement the minimal shell without absorbing product authority.
**Predecessors:** `CLI0`.
**Subblocks:** (1) command/service registry; (2) captured workspace/config/target selection; (3) versioned invocation/result envelope; (4) cancellation/signals/concurrency; (5) human, JSON, SARIF where applicable, and quiet reporters; (6) protocol isolation for `lsp`/`mcp`; (7) unit/security/performance tests.
**Acceptance:** services can register independently; machine stdout is uncontaminated; invalid/missing/ambiguous targets are typed; shell startup and no-work paths meet locked gates.
**Forbidden:** importing product internals, parsing semantic results in reporters, ambient config, or one process-global mutable session.
**Deletion/abort:** replace duplicate argument/reporting infrastructure only after parity; abort if a command requires shell-specific semantic logic.
~~~~
