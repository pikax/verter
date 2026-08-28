# Exact operative source-clause attachment — CLIC0

Schema: 1. Node: `CLIC0`. Clause count: 8. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-COMP-L1961-AF01A6A69146

- Kind: `context`; source: `compiler-proposal.md:1961-1961`; target: `node:CLIC0`; text SHA-256: `af01a6a69146c52bdc4084095c59b70e6f18d8c2302fb4ddbd2963a56dc3c34b`.

~~~~markdown
## 11.7 `CLIC0`
~~~~

### SRC-COMP-L1963-77E358A8D925

- Kind: `deletion`; source: `compiler-proposal.md:1963-1963`; target: `node:CLIC0`; text SHA-256: `77e358a8d92569bb4d248f062ac55e97e5e853e8bf199cec9d95eabac5ed46f4`.

~~~~markdown
`CLIC0` consumes the CCA2 `CompileArtifactSet` and exact runtime-compiler capability. It remains able to expose existing Vue/Svelte compilers before V2, through temporary adapters. VCP6/SCP6 later delete those adapters without changing CLI command semantics.
~~~~

### SRC-COMP-L1965-F05E79FB27F2

- Kind: `context`; source: `compiler-proposal.md:1965-1965`; target: `node:CLIC0`; text SHA-256: `f05e79fb27f2c9cb052d0d8384b6ecfc475c9d0964f7335b53bdf3384b8e3d42`.

~~~~markdown
The command exposes:
~~~~

### SRC-COMP-L1967-02562180FDCE

- Kind: `context`; source: `compiler-proposal.md:1967-1971`; target: `node:CLIC0`; text SHA-256: `02562180fdce9de240d838e7176971298a21a500f9a31689a3a1b8d1fff51886`.

~~~~markdown
```text
Supported
FutureSeparateTrain
NotApplicable
```
~~~~

### SRC-COMP-L1973-180203AC91A2

- Kind: `requirement`; source: `compiler-proposal.md:1973-1973`; target: `node:CLIC0`; text SHA-256: `180203ac91a2dc538a01168dce667dc4c336dae87528ca2f197ccab9ddfa6556`.

~~~~markdown
and exposes `Optimized` only when its capability is actually accepted.
~~~~

### SRC-COMP-L1975-F52D711103D5

- Kind: `context`; source: `compiler-proposal.md:1975-1975`; target: `node:CLIC0`; text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`.

~~~~markdown
---
~~~~

### SRC-EXP-L1511-7DD5F0DE8A40

- Kind: `context`; source: `successor-expansion.md:1511-1511`; target: `node:CLIC0`; text SHA-256: `7dd5f0de8a40d84f770de5c1fbc8f24c18bfb33c8efe3041293210fcdf3907e7`.

~~~~markdown
### `CLIC0.md` — Registered carrier `compile` command
~~~~

### SRC-EXP-L1513-7C69D751D4A7

- Kind: `forbidden`; source: `successor-expansion.md:1513-1518`; target: `node:CLIC0`; text SHA-256: `7c69d751d4a7f2c274778f311dc6b899c37293eae06d9059fdf83ab2acf6c015`.

~~~~markdown
**Intent:** route compilation only to optional Verter-owned compiler backends while keeping tooling-only carriers first-class.
**Predecessors:** `CLI1`, `CPF1`.
**Subblocks:** (1) resolve exact carrier/backend capability; (2) route Vue/Svelte SFC compilation; (3) return normalized `Supported | FutureSeparateTrain | NotApplicable`; (4) write output/map manifests atomically; (5) project/reference/watch selection; (6) differential/cancellation/performance tests.
**Acceptance:** Vue/Svelte preserve admitted compiler bytes/maps; Astro returns `FutureSeparateTrain`; HTML/MDX and other non-compiler carriers return `NotApplicable`; tooling availability is unaffected.
**Forbidden:** compiler stubs for every carrier, runtime ownership, treating tooling support as compilation, or generic “unsupported” that loses disposition.
**Deletion/abort:** migrate old compile shells only after parity; abort any backend without source-map and atomic-output guarantees.
~~~~
