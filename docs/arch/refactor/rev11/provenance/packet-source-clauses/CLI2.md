# Exact operative source-clause attachment — CLI2

Schema: 1. Node: `CLI2`. Clause count: 2. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-EXP-L1493-9D8415D01B7E

- Kind: `context`; source: `successor-expansion.md:1493-1493`; target: `node:CLI2`; text SHA-256: `9d8415d01b7eaa3440de6a70868034db0f62cc478394a4107a7e030cb8ceebc5`.

~~~~markdown
### `CLI2.md` — Verter-native `typecheck` command
~~~~

### SRC-EXP-L1495-5096FB413805

- Kind: `forbidden`; source: `successor-expansion.md:1495-1500`; target: `node:CLI2`; text SHA-256: `5096fb41380518ce3b077d4b5e9faa67c797206cebf7a561995d1a2fa5048516`.

~~~~markdown
**Intent:** expose the composed Verter diagnostic plan as a non-emitting command distinct from the TypeScript-compatible driver.
**Predecessors:** `CLI1`, `TIF0`.
**Subblocks:** (1) select exact carrier/framework/project profiles; (2) compose only native/framework type diagnostics and certified TypeScript observations according to their owners; (3) return provenance/completeness/NeedInputs; (4) enforce zero filesystem writes and exclude lint/formatting; (5) project/reference/watch inputs; (6) incremental/fresh/differential/performance tests.
**Acceptance:** `verter typecheck` means Verter’s composed native/framework/TS diagnostic plan and writes nothing; it is not an alias for `tsc --noEmit`; unavailable owners produce truthful partial/NeedInputs results.
**Forbidden:** emit, CLI-owned diagnostics, creating a second TS program, silently selecting the first project, or collapsing partiality to success.
**Deletion/abort:** replace only the old typecheck shell after service parity; abort if any diagnostic lacks an exact owner/basis.
~~~~
