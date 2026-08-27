# Exact operative source-clause attachment — CPF1

Schema: 1. Node: `CPF1`. Clause count: 7. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-COMP-L1901-98B1EF372E16

- Kind: `context`; source: `compiler-proposal.md:1901-1901`; target: `node:CPF1`; text SHA-256: `98b1ef372e16ee003894d2d3e34c8dc96c86c5c2198e7ff3ac57c117c47bacdd`.

~~~~markdown
## 11.2 `CPF1`
~~~~

### SRC-COMP-L1903-ABE8AD244987

- Kind: `deletion`; source: `compiler-proposal.md:1903-1903`; target: `node:CPF1`; text SHA-256: `abe8ad244987cba3a4d39f2dde5ec8149676a9c7004a63074428543c7fec0f07`.

~~~~markdown
Make `CPF1` the successor catalog integration and temporary-bridge deletion owner, not a second carrier split.
~~~~

### SRC-COMP-L1905-62223B1E2571

- Kind: `context`; source: `compiler-proposal.md:1905-1905`; target: `node:CPF1`; text SHA-256: `62223b1e25718add41f2fd094f5bb595ce9cce321a9500976c31119be2656df0`.

~~~~markdown
Typed tables should include:
~~~~

### SRC-COMP-L1907-BBDD00122625

- Kind: `context`; source: `compiler-proposal.md:1907-1913`; target: `node:CPF1`; text SHA-256: `bbdd001226256fa5eb1d6f25bc6ba282bb969a1c8ff630055ec615225ff53f2e`.

~~~~markdown
```text
carrier_frontends
framework_semantic_authorities
projection_backends
runtime_compilers
framework_host_integrations
```
~~~~

### SRC-COMP-L1915-B175F5D7AFBB

- Kind: `requirement`; source: `compiler-proposal.md:1915-1915`; target: `node:CPF1`; text SHA-256: `b175f5d7afbb9fcd687a1161fa87298a9f6e707aa4c3a07c663a47f49566f9b7`.

~~~~markdown
Only one immutable catalog authority is permitted.
~~~~

### SRC-EXP-L806-6610DA9294BB

- Kind: `context`; source: `successor-expansion.md:806-806`; target: `node:CPF1`; text SHA-256: `6610da9294bb730afc4a7df299718b925fedb54ab88f2f4e2440a010d43ccdd9`.

~~~~markdown
### `CPF1.md` — Carrier frontend registration and Vue/Svelte cutover
~~~~

### SRC-EXP-L808-EED3D39EE390

- Kind: `forbidden`; source: `successor-expansion.md:808-813`; target: `node:CPF1`; text SHA-256: `eed3d39ee390f8a0844f58128cdd0957591d1a02b51bd0977756040537bfd8ab`.

~~~~markdown
**Intent:** atomically install the frontend/backend split and migrate current carriers.
**Predecessors:** `CPF0`, `CAT0`.
**Subblocks:** (1) add `CarrierFrontendRegistry`; (2) add optional `CarrierCompilerBackendRegistry`; (3) migrate Vue/Svelte parse, source-unit, IDE-projection, fact, and compile routes; (4) replace central `CarrierGrammarConfig::{Vue,Svelte}` with owner-local typed configs; (5) update generated client and capability guards; (6) delete the combined registry/trait.
**Acceptance:** Vue/Svelte authored bytes, parse facts, recovery, IDE projection, maps, compilation, cache hits, and public outputs are equivalent on pinned corpora; “all carriers have a frontend, only compile-capable carriers require a backend” is mechanically exhaustive.
**Forbidden:** dual-running registries, public erased artifacts, central grammar switches, or a compatibility bridge that becomes an authority.
**Deletion/abort:** combined compiler registry/trait and stale guards are deleted atomically; abort on unexplained output/map/performance divergence.
~~~~
