# Exact operative source-clause attachment — CLI4

Schema: 1. Node: `CLI4`. Clause count: 2. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-EXP-L1520-ED681F48EFEC

- Kind: `context`; source: `successor-expansion.md:1520-1520`; target: `node:CLI4`; text SHA-256: `ed681f48efec682e0dddd7b6e50122b8bf3f962a283dab2ac0ab65cbe920daa6`.

~~~~markdown
### `CLI4.md` — `type-info`, `lsp`, and `mcp` command adapters
~~~~

### SRC-EXP-L1522-827FB1EBE47E

- Kind: `forbidden`; source: `successor-expansion.md:1522-1527`; target: `node:CLI4`; text SHA-256: `827fb1ebe47e83b6014b0b8a2af60541749d111eeeb26bc693f6e27a28b655d8`.

~~~~markdown
**Intent:** expose TypeInfo and managed protocols without duplicating their services.
**Predecessors:** `CLI1`, `TIF1`.
**Subblocks:** (1) mutually exclusive `type-info` selectors: file+byte offset, file+`LINE:CHAR`, file+name, and bounded project/workspace name; (2) require an explicit UTF-8/UTF-16/UTF-32 encoding for one-based human `LINE:CHAR` and keep machine positions structured/zero-based; (3) stable candidates/NeedSelection human and versioned JSON output; (4) `lsp` stdio/socket lifecycle; (5) `mcp` stdio/HTTP lifecycle as admitted; (6) cancellation/security/protocol-output tests.
**Acceptance:** every selector calls one TypeInfo service and reports basis/completeness; ambiguous name never picks first; LSP/MCP stdout remains protocol-clean and lifecycle-correct.
**Forbidden:** CLI-created TS programs, position defaults without a contract, provider handles in JSON, or server semantics inside the shell.
**Deletion/abort:** old lsp/mcp/type-info shells become wrappers only at `CLI5`; abort on protocol leakage.
~~~~
