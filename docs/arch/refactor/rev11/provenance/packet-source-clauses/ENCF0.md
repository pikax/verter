# Exact operative source-clause attachment — ENCF0

Schema: 1. Node: `ENCF0`. Clause count: 2. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

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
