# Exact operative source-clause attachment — FMT4

Schema: 1. Node: `FMT4`. Clause count: 2. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-EXP-L1390-877B3B6566DF

- Kind: `context`; source: `successor-expansion.md:1390-1390`; target: `node:FMT4`; text SHA-256: `877b3b6566df09580c01f634b62cf680e5eb121554dc7c57b2789f0d1e18a434`.

~~~~markdown
### `FMT4.md` — Formatter LSP/public parity, conformance, and promotion
~~~~

### SRC-EXP-L1392-250320FF6ACD

- Kind: `forbidden`; source: `successor-expansion.md:1392-1397`; target: `node:FMT4`; text SHA-256: `250320ff6acd9f3e2d51b825ee42df59b454ce35b4be16e9ea3b357afcd8cfcf`.

~~~~markdown
**Intent:** expose and independently promote the formatter across all applicable surfaces.
**Predecessors:** `FMT3`, `PUB0`, `PER0`.
**Subblocks:** (1) Rust/NAPI/WASM request/result; (2) LSP document/range/on-type cells where applicable; (3) MCP formatting service cells; (4) config/ignore/override provenance; (5) cold/warm/large-file/RSS/cancellation/zero-work tests; (6) dogfood and exact-candidate reviews.
**Acceptance:** Rust/NAPI/WASM/LSP/MCP surfaces agree on output/edits/maps; LSP capability is registered only under its ownership mask; repository dogfood produces a reviewed finite diff; CLI remains explicitly unavailable until `CLIF0`; formatter maturity promotes independently.
**Forbidden:** waiting for future verticals, hiding unsupported custom blocks, or using lint fixes to make formatter conformance pass.
**Deletion/abort:** delete only named obsolete public formatter façade APIs/packages assigned to `FMT4` by the `UAK0` ledger after zero-consumer/generated-reference proof; printer and routing deletions remain with their earlier sole owners. Any failing cell returns to its printer/composition owner.
~~~~
