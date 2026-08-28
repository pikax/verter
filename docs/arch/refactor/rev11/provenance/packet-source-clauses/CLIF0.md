# Exact operative source-clause attachment — CLIF0

Schema: 1. Node: `CLIF0`. Clause count: 2. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-EXP-L1538-F0B895F9A6BD

- Kind: `context`; source: `successor-expansion.md:1538-1538`; target: `node:CLIF0`; text SHA-256: `f0b895f9a6bd21a70356517864a15c220ba5f2b848e697d9ccf681b86f085a18`.

~~~~markdown
### `CLIF0.md` — Formatter CLI adapter
~~~~

### SRC-EXP-L1540-F1EFA702B87D

- Kind: `forbidden`; source: `successor-expansion.md:1540-1545`; target: `node:CLIF0`; text SHA-256: `f1efa702b87d91dc023d0882db0aa8606df839b33d4d3fb6a7ae374b6456d446`.

~~~~markdown
**Intent:** add `verter fmt` as a thin adapter over the independently promoted formatter service.
**Predecessors:** `CLI1`, `FMT4`.
**Subblocks:** (1) file/project/stdin selection; (2) `--check` and `--write`; (3) range/encoding/config/ignore provenance; (4) human/JSON reporters; (5) atomic multi-file writes and stale validation; (6) watch/performance/cancellation tests.
**Acceptance:** `fmt --check` never writes; `--write` commits one validated transaction; output/edits/maps match the formatter service; no formatting semantics live in CLI.
**Forbidden:** external formatter invocation, CLI-owned options, per-file partial commit, lint fixes, or hidden unsupported success.
**Deletion/abort:** delete standalone formatter shell adapters only after parity and zero consumers; abort without recoverable atomic writes.
~~~~
