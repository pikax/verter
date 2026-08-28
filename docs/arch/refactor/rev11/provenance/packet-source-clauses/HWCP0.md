# Exact operative source-clause attachment — HWCP0

Schema: 1. Node: `HWCP0`. Clause count: 2. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-EXP-L1159-451351A9CE4C

- Kind: `context`; source: `successor-expansion.md:1159-1159`; target: `node:HWCP0`; text SHA-256: `451351a9ce4cfe25f83f1ccf7f9ecd5b0f4d1d1db3f83acdaeb5e670e6125e84`.

~~~~markdown
### `HWCP0.md` — HTML/WC public-surface adapters
~~~~

### SRC-EXP-L1161-C11084860012

- Kind: `forbidden`; source: `successor-expansion.md:1161-1166`; target: `node:HWCP0`; text SHA-256: `c1108486001248a13e6698a2c0d8b97cb5de5792788029d467ce41df404ae572`.

~~~~markdown
**Intent:** expose one semantic implementation across applicable non-CLI surfaces with exact maturity.
**Predecessors:** `HWC2`, `HWC3`, `PUB0`.
**Subblocks:** (1) Rust requests/results; (2) NAPI; (3) WASM prepared-input boundary; (4) LSP adapter; (5) MCP resource/tool cells; (6) generated capability matrix and cross-surface differential tests.
**Acceptance:** available surfaces return equivalent facts/provenance/partiality; filesystem-requiring WASM operations return `NeedInputs`; CLI is explicitly unavailable until its adapter train, not falsely “at parity.”
**Forbidden:** a surface-specific semantic DTO, implicit encoding, CLI code, compiler stubs, or a blanket first-class claim hiding unavailable cells.
**Deletion/abort:** remove superseded public adapters only after generated-consumer parity; abort a surface cell that cannot truthfully supply required inputs.
~~~~
