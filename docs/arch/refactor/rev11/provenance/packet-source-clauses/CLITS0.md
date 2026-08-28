# Exact operative source-clause attachment — CLITS0

Schema: 1. Node: `CLITS0`. Clause count: 2. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-EXP-L1502-D2228B5C25D3

- Kind: `context`; source: `successor-expansion.md:1502-1502`; target: `node:CLITS0`; text SHA-256: `d2228b5c25d3bc4a6e8ba823f725a42eaafe2e515e130e73e8c5cee7c830e764`.

~~~~markdown
### `CLITS0.md` — TypeScript-compatible `tsc` command
~~~~

### SRC-EXP-L1504-F8066DD5D3D3

- Kind: `forbidden`; source: `successor-expansion.md:1504-1509`; target: `node:CLITS0`; text SHA-256: `f8066dd5d3d3b07d53243d67f778dfffaae307eac477baa8926a75d2924b3f7a`.

~~~~markdown
**Intent:** expose a certified TypeScript-compatible driver, including `--noEmit`, without redefining TypeScript semantics.
**Predecessors:** `CLI1`.
**Subblocks:** (1) bind the selected certified TypeScript engine; (2) project admitted Verter carriers through the accepted TCM plane; (3) support project/reference/watch selection; (4) preserve TypeScript flags/diagnostics/exit semantics; (5) perform declaration/JS emit through the certified engine with atomic writes; (6) differential and performance corpus.
**Acceptance:** `tsc --noEmit` follows the certified TypeScript driver rather than Verter’s composed typecheck plan; emitting modes match locked TypeScript behavior; backend/project/snapshot identity is exact.
**Forbidden:** native reimplementation of TypeScript checks/emit, another TS program, Verter runtime codegen, or partial output commit.
**Deletion/abort:** convert the old `verter-tsc` entry point to a wrapper only at `CLI5`; abort an emit path lacking atomic commit.
~~~~
