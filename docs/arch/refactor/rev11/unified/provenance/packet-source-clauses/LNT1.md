# Exact operative source-clause attachment — LNT1

Schema: 1. Node: `LNT1`. Clause count: 2. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-EXP-L1428-BF92B4446DA2

- Kind: `context`; source: `successor-expansion.md:1428-1428`; target: `node:LNT1`; text SHA-256: `bf92b4446da2c320c4f945ff550da35252a6309b257553a4fec5611b6a3594b7`.

~~~~markdown
### `LNT1.md` — JS/TS and TypeScript-ESLint compatibility pack
~~~~

### SRC-EXP-L1430-5D3E0AB3491A

- Kind: `forbidden`; source: `successor-expansion.md:1430-1435`; target: `node:LNT1`; text SHA-256: `5d3e0ab3491adadab612f21b017807ae5c1031cc4745630a97d8a10478bb144c`.

~~~~markdown
**Intent:** close the highest-value pinned host-language rule cells without absorbing framework rules.
**Predecessors:** `LNT2`.
**Subblocks:** (1) syntax-only JS correctness/security cells; (2) certified-TypeScript-aware cells; (3) common performance/maintainability cells; (4) suppression/severity/config parity; (5) safe/suggested fix corpus; (6) differential false-positive, zero-work, and allocation/latency tests.
**Acceptance:** each admitted cell matches locked meaning, range, severity/config, and fix behavior; rules requiring certified TS facts state exact basis; inapplicable rules allocate/do no work.
**Forbidden:** framework switches, native recreation of TS-authoritative facts, regex where parsed facts are required, or lowering a cell after implementation.
**Deletion/abort:** delete only named common-rule rows after parity; shared registry deletion belongs to `LNT3`; genuinely different behavior is labeled Verter-only.
~~~~
