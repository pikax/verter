# Exact operative source-clause attachment — FMTH0

Schema: 1. Node: `FMTH0`. Clause count: 2. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-EXP-L1132-D2E90B968296

- Kind: `context`; source: `successor-expansion.md:1132-1132`; target: `node:FMTH0`; text SHA-256: `d2e90b968296a7dd38c168eeb2cd5293c44b28c8e77c28ea5a605527a1ff5ac4`.

~~~~markdown
### `FMTH0.md` — Native neutral-HTML formatter
~~~~

### SRC-EXP-L1134-75EE2ABBE9A0

- Kind: `forbidden`; source: `successor-expansion.md:1134-1139`; target: `node:FMTH0`; text SHA-256: `75ee2abbe9a0e493a8b17a3bb614bde764afb2b2bd4708eea364f8a0cf9320ac`.

~~~~markdown
**Intent:** implement the neutral HTML full/range printer on the already-locked formatter substrate before any SFC composition.
**Predecessors:** `FMT1`, `FCFG0`, `HWC2`, `PUB0`, `PER0`.
**Subblocks:** (1) HTML format view and authored trivia; (2) element/attribute/text/comment/raw-text printers; (3) malformed/recovery islands; (4) range/cursor/edit/`FormatPositionMap` behavior; (5) Prettier differential and idempotence corpus; (6) Rust/NAPI/WASM service plus performance/cancellation tests.
**Acceptance:** locked exact cells are byte-equivalent, divergences are predeclared, repeated formatting stabilizes, malformed retained bytes and every edit map exactly, and no Vue/Svelte branch exists.
**Forbidden:** delegating to Prettier/oxfmt, Vue parser semantics, whole-file replacement when smaller edits are proven, or deleting an SFC formatter path.
**Deletion/abort:** delete only superseded neutral-HTML formatter code after zero callers; abort a compatibility cell rather than fabricate parity.
~~~~
