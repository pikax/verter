# Exact operative source-clause attachment — DEM0

Schema: 1. Node: `DEM0`. Clause count: 4. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-COMP-L1931-9A01AF6D6916

- Kind: `context`; source: `compiler-proposal.md:1931-1931`; target: `node:DEM0`; text SHA-256: `9a01af6d6916a11bb55139d33d226aa11a4df3c89c7b5d9d56a6a132b5291c97`.

~~~~markdown
## 11.4 `DEM0`
~~~~

### SRC-COMP-L1933-0CEF8F108AEE

- Kind: `context`; source: `compiler-proposal.md:1933-1933`; target: `node:DEM0`; text SHA-256: `0cef8f108aee5c05cedd470828b7fdc48f80ac0bac69f4b1613f0d1d916b0b3b`.

~~~~markdown
Expose a finite reasoned demand-closure primitive usable by compiler specialization. Compiler demand may add framework-specific fact/target capabilities but may not create a second generic demand engine.
~~~~

### SRC-EXP-L869-729B506598F6

- Kind: `context`; source: `successor-expansion.md:869-869`; target: `node:DEM0`; text SHA-256: `729b506598f6d8914a027db98b353b6f40dd4f8468e17154e8580152603317e8`.

~~~~markdown
### `DEM0.md` — Selection, two-stage activation, and demand planning
~~~~

### SRC-EXP-L871-F5E9A3D34C1F

- Kind: `forbidden`; source: `successor-expansion.md:871-876`; target: `node:DEM0`; text SHA-256: `f5e9a3d34c1fda89f2980fe83caa27d4d61f0370806458a15f09bafc36e77f7c`.

~~~~markdown
**Intent:** ensure supported profiles remain dormant until proven and requested.
**Predecessors:** `CAT0`, `VID0`, `CFG0`.
**Subblocks:** (1) define captured selection inputs; (2) define pre-projection `SourceActivationPlan`; (3) define post-snapshot `SemanticClaimPlan`; (4) define capability-level `CapabilityDemandPlan`; (5) define conflict/ambiguity resolution and epoch transitions; (6) audit zero-work and cancellation.
**Acceptance:** disabled, selected-but-unrequested, ambiguous, missing-package, and rapid-mode-change fixtures show exact work/audit outcomes; post-snapshot facts cannot mutate the current parse/transform generation.
**Forbidden:** semantic-oracle calls from activation, eager all-capability execution, ambient package/config reads, or spelling-based framework activation.
**Deletion/abort:** remove legacy eager/one-framework-per-file selectors after parity; abort if a capability cannot state its exact fact demands.
~~~~
