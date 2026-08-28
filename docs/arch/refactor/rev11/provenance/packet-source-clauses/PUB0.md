# Exact operative source-clause attachment — PUB0

Schema: 1. Node: `PUB0`. Clause count: 2. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-EXP-L977-E368C210381D

- Kind: `context`; source: `successor-expansion.md:977-977`; target: `node:PUB0`; text SHA-256: `e368c210381d53bbf1e8dd7831fbb87345b0a72d733682b355a11cf0e3cdeacf`.

~~~~markdown
### `PUB0.md` — Versioned public request/result and capability truth
~~~~

### SRC-EXP-L979-AC3D83A7573A

- Kind: `forbidden`; source: `successor-expansion.md:979-984`; target: `node:PUB0`; text SHA-256: `ac3d83a7573a43dfb9d627131ddf8df319ad99bcf1346efbf963e9eb9ee500b7`.

~~~~markdown
**Intent:** make Rust, NAPI, WASM, LSP, MCP, and CLI consumers observe one semantic vocabulary and honest availability.
**Predecessors:** `ENC1`, `TIF1`, `LRA0`, `FMK0`, `COX0`, `PER0`.
**Subblocks:** (1) request/result envelope and schema epochs; (2) typed success/partial/ambiguous/NeedInputs/unsupported/not-applicable/cancelled/stale outcomes; (3) generated per-surface capability/maturity matrix; (4) prepared-input and filesystem boundaries; (5) cancellation/budget/encoding propagation; (6) compatibility and reserved-field policy.
**Acceptance:** differential fixtures return equivalent semantic facts across available surfaces; WASM reports missing inputs rather than empty success; LSP registers only full-participation applicable capabilities.
**Forbidden:** surface-specific semantic DTOs, boolean capability lies, implicit encoding, provider handles, or CLI presentation fields in core results.
**Deletion/abort:** delete duplicate public envelopes only after generated consumer parity; rescope when a surface cannot supply required inputs and mark the capability accordingly.
~~~~
