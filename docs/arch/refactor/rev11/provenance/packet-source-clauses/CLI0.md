# Exact operative source-clause attachment — CLI0

Schema: 1. Node: `CLI0`. Clause count: 3. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-EXP-L1473-0C2D64660CD7

- Kind: `context`; source: `successor-expansion.md:1473-1473`; target: `contract:contracts/sizing.md`; text SHA-256: `0c2d64660cd765790b283044d3fb2305afcff65b24464e3ea5a93e6c9cda3bab`.

~~~~markdown
## 14. Unified `verter` CLI train
~~~~

### SRC-EXP-L1475-F0EEA8C93A02

- Kind: `context`; source: `successor-expansion.md:1475-1475`; target: `node:CLI0`; text SHA-256: `f0eea8c93a028d7e7c32e227867675746d91882f974861eac1073aaf1b553825`.

~~~~markdown
### `CLI0.md` — `verter` command/package and semantic lock
~~~~

### SRC-EXP-L1477-B9AE364E57FA

- Kind: `forbidden`; source: `successor-expansion.md:1477-1482`; target: `node:CLI0`; text SHA-256: `b9ae364e57fa2d6ab7a59a7e759c4840f059dcde523247c11bc9dd98518670fe`.

~~~~markdown
**Intent:** freeze one executable surface and distinct command semantics before building the shell.
**Predecessors:** `PUB0`.
**Subblocks:** (1) resolve `@verter/cli` package and `verter` binary naming, including private root-package collision; (2) lock command grammar/exit codes/stdout/stderr/machine schemas; (3) distinguish `typecheck`, `tsc`, `compile`, `type-info`, service-host, formatter, and lint command families; (4) normalize compiler disposition to `Supported | FutureSeparateTrain | NotApplicable`; (5) inventory existing binaries/packages and consumers; (6) lock one-release wrapper policy, later deletion receipt, and performance/security gates.
**Acceptance:** every command maps to an existing or separately planned service owner; no placeholder/no-op command is admitted; package ownership and cutover are explicit.
**Forbidden:** one “check” semantic hiding emit/mutation, CLI-owned analyzers, indefinite aliases, or unscoped package assumptions.
**Deletion/abort:** no code; omit any command lacking a truthful engine rather than ship a placeholder.
~~~~
