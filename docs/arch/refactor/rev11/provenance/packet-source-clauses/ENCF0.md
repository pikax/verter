# Exact operative source-clause attachment — ENCF0

Schema: 1. Node: `ENCF0`. Clause count: 4. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-EXP-L851-B212E70636E1

- Kind: `context`; source: `successor-expansion.md:851-851`; target: `node:ENCF0`; text SHA-256: `b212e70636e12c28a5b81699f3101076ad1d581214bb6b2adfb30a223a53a99a`.

~~~~markdown
### `ENCF0.md` — NAPI, WASM, FFI, MCP, and CLI coordinate-boundary cutover
~~~~

### SRC-EXP-L853-EAFE3F5F52B9

- Kind: `forbidden`; source: `successor-expansion.md:853-858`; target: `node:ENCF0`; text SHA-256: `eafe3f5f52b9c6282f44ed9f447e376fe24ad169cd37722f7265ee4e2f602b88`.

~~~~markdown
**Intent:** give every non-editor public boundary explicit coordinate and line/column semantics.
**Predecessors:** `ENC0`.
**Subblocks:** (1) versioned encoding tags for NAPI/WASM/FFI/MCP; (2) CLI `--offset` and `LINE:CHAR` selectors; (3) lock human CLI line/character as one-based code-unit coordinates in an explicitly selected UTF-8/UTF-16/UTF-32 encoding; (4) convert requests/results/edits/maps at adapters; (5) prepared-input and invalid-boundary behavior; (6) cross-surface differential and allocation tests.
**Acceptance:** untagged API positions fail schema validation; CLI examples are unambiguous; every surface returns the same authored location after declared conversion; Rust facts remain UTF-8 bytes.
**Forbidden:** surface-specific semantic positions, hidden zero/one-based defaults, lossy conversion, or requester encoding in native facts.
**Deletion/abort:** delete fixed-encoding binding fields only after generated consumers migrate; abort when an ABI cannot version its coordinate contract.
~~~~

### SRC-LEGACY-TRANSFER-5EB276F91540

- Kind: `requirement`; source: `legacy-architecture-transfers.md:138-143`; target: `node:G5`; text SHA-256: `772948391d6b3ec941015dcbbe756cb7482eecd7f00b9d618a203d4f750580ac`.

~~~~markdown
### LEGACY-TRANSFER-5EB276F91540

- Original path: `docs/arch/future/napi-wasm-async-boundary.md`; Git blob: `5eb276f91540d8dba52f9ea3bc7660fa1be8e34a`; exact source SHA-256: `fbf4928d657b5c083d25c0d4dccc160f3930816236c31b8a5f9e7dfe50dea6a3`.
- Exact retained source: `sources/legacy-architecture-transfers/future/napi-wasm-async-boundary.md`.
- Applicable authority: `G5`, `ENCF0`, `NCK7`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~

### SRC-LEGACY-TRANSFER-BEE69A58A481

- Kind: `requirement`; source: `legacy-architecture-transfers.md:544-549`; target: `node:B6`; text SHA-256: `becb56c3921c93fa05c4ffa12ef83d38451fb4f4bc3ad0d2aa0c3d15d0ab81d1`.

~~~~markdown
### LEGACY-TRANSFER-BEE69A58A481

- Original path: `docs/arch/stage10-b6-ffi-output-materialization.md`; Git blob: `bee69a58a481f2e81bde732268921beb5b942ab0`; exact source SHA-256: `369328569f40438d89e090e33f08ee6b9c4b06f6b4ac359d93877ad45c9ad5e2`.
- Exact retained source: `sources/legacy-architecture-transfers/stage10-b6-ffi-output-materialization.md`.
- Applicable authority: `B6`, `ENCF0`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~
