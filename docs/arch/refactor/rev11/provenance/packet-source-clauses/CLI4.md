# Exact operative source-clause attachment — CLI4

Schema: 1. Node: `CLI4`. Clause count: 5. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-EXISTING-NODE-AMENDMENT-CLI4

- Kind: `requirement`; source: `existing-node-amendments.md:222-229`; target: `node:CLI4`; text SHA-256: `3f9560f6edb4b733d3c18ae4ad62aa10d5cc8df84d1d4507193f5b87121291fc`.

~~~~markdown
## CLI4 — type-info, lsp, and mcp adapters

When opened:

- expose thin adapters to shared diagnostic, language-service, and engine status services;
- preserve core request/result identity and outcomes;
- do not add command-local provider discovery, capability, mapping, or semantic DTOs;
- engine acquisition commands, if ever exposed, must be explicit EPR2 side-effect requests and never ordinary `lsp` startup behavior.
~~~~

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

### SRC-LEGACY-TRANSFER-12BFE5263E5E

- Kind: `requirement`; source: `legacy-architecture-transfers.md:131-136`; target: `node:G5`; text SHA-256: `33b198cd1a8829e34f377da38bd4b1691d8809952cc2dfa8962119be6501a1bf`.

~~~~markdown
### LEGACY-TRANSFER-12BFE5263E5E

- Original path: `docs/arch/future/mcp-tool-async-state-machine-sizes.md`; Git blob: `12bfe5263e5ee2b4452440a1adf08a4721f3496f`; exact source SHA-256: `4120f46f81c5101411e8b2429b4334b565971eeeeb8cee7bcb354b8bf1b4547e`.
- Exact retained source: `sources/legacy-architecture-transfers/future/mcp-tool-async-state-machine-sizes.md`.
- Applicable authority: `G5`, `CLI4`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~

### SRC-LEGACY-TRANSFER-8F410E617939

- Kind: `requirement`; source: `legacy-architecture-transfers.md:89-94`; target: `node:LSO10`; text SHA-256: `6736d109f9b043fff7119dc652006046642673d1a0f10ef5aa25cd7e8c1554a8`.

~~~~markdown
### LEGACY-TRANSFER-8F410E617939

- Original path: `docs/arch/future/ide-compile-synchronous-extras.md`; Git blob: `8f410e617939b6f6079060b84e57bb3974fbb5f2`; exact source SHA-256: `67b125580cf3a5a8158af4098a89182e5e0ebf8078e695cf91a113973ced0a0a`.
- Exact retained source: `sources/legacy-architecture-transfers/future/ide-compile-synchronous-extras.md`.
- Applicable authority: `LSO10`, `CLI4`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~
