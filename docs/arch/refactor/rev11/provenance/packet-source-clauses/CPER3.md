# Exact operative source-clause attachment — CPER3

Schema: 1. Node: `CPER3`. Clause count: 18. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-COMP-L1781-C8169BF2453F

- Kind: `context`; source: `compiler-proposal.md:1781-1781`; target: `node:CPER3`; text SHA-256: `c8169bf2453fad8ea0a29ed7f4e20e44de4a3a85482f2e151e87ee2f4c8ba209`.

~~~~markdown
## `CPER3.md` — Cross-framework compiler soak and equivalent-work study
~~~~

### SRC-COMP-L1783-EB613B74AB42

- Kind: `context`; source: `compiler-proposal.md:1783-1783`; target: `node:CPER3`; text SHA-256: `eb613b74ab42ffabd2d75030bc8b11ede0e4cf4a999a843cd7cb872c352d901c`.

~~~~markdown
**Intent:** measure the mature common engine and both compilers under long-running, mixed, multi-target, incremental and concurrent workloads.
~~~~

### SRC-COMP-L1785-14488F6A61DF

- Kind: `context`; source: `compiler-proposal.md:1785-1785`; target: `node:CPER3`; text SHA-256: `14488f6a61df476d4df239b7bce9b172680fb8d5ac2630ee91c47705b48d94e9`.

~~~~markdown
**Problem:** independent product benchmarks do not expose shared-engine RSS, allocator, scheduler, cache or mixed-workspace pathologies.
~~~~

### SRC-COMP-L1787-743589F4F472

- Kind: `context`; source: `compiler-proposal.md:1787-1787`; target: `node:CPER3`; text SHA-256: `743589f4f47213c45f6d7d2d028407bf10b76cf8cd6409b5ef33e2d9b6a50aca`.

~~~~markdown
**Solution and architecture decisions:** non-release soak covering:
~~~~

### SRC-COMP-L1789-FDF935CB17F8

- Kind: `context`; source: `compiler-proposal.md:1789-1789`; target: `node:CPER3`; text SHA-256: `fdf935cb17f8a14909e0be6dbcabfc1da8d4916f47f7afad9970e67c174fc081`.

~~~~markdown
- mixed Vue/Svelte batches;
~~~~

### SRC-COMP-L1790-53DBE7DE406E

- Kind: `context`; source: `compiler-proposal.md:1790-1790`; target: `node:CPER3`; text SHA-256: `53dbe7de406ed0edcbed1e357114a8c288394c6e2c1ef47aa03ba03419a139e9`.

~~~~markdown
- client/server or VDOM/SSR/Vapor multi-target sharing;
~~~~

### SRC-COMP-L1791-A6286F39B38B

- Kind: `context`; source: `compiler-proposal.md:1791-1791`; target: `node:CPER3`; text SHA-256: `a6286f39b38b66cd3c73b1be60c4e790aa1116de26064007e6b8f31dcb5ee5eb`.

~~~~markdown
- maps/no maps;
~~~~

### SRC-COMP-L1792-B6B532F355F7

- Kind: `context`; source: `compiler-proposal.md:1792-1792`; target: `node:CPER3`; text SHA-256: `b6b532f355f7d6c17b7ebec8f2482a855358d0cf6a61381d44bee196774c9b7d`.

~~~~markdown
- direct/prepared/managed execution;
~~~~

### SRC-COMP-L1793-2639400E56A4

- Kind: `context`; source: `compiler-proposal.md:1793-1793`; target: `node:CPER3`; text SHA-256: `2639400e56a40c86b1f941d3e47002468cf73e3dc953cff4178bec1dad6ae0b0`.

~~~~markdown
- edit storms, cancellation and stale-result rejection;
~~~~

### SRC-COMP-L1794-E397EEC55D52

- Kind: `context`; source: `compiler-proposal.md:1794-1794`; target: `node:CPER3`; text SHA-256: `e397eec55d526a9ac7054ea025279a9db5fa73f80470be3a9873aff54d2f892a`.

~~~~markdown
- long-session RSS plateau and idle CPU;
~~~~

### SRC-COMP-L1795-561A971AFD16

- Kind: `context`; source: `compiler-proposal.md:1795-1795`; target: `node:CPER3`; text SHA-256: `561a971afd16db2ab4232e2fdf9b8c88756ea531f15c9a9d88653c7676c1ce9d`.

~~~~markdown
- small-file batching and large-component thresholds;
~~~~

### SRC-COMP-L1796-3AEDB7786370

- Kind: `context`; source: `compiler-proposal.md:1796-1796`; target: `node:CPER3`; text SHA-256: `3aedb7786370abac194c17d92ebf757209bd6435f800a9adbf70c04ccea1a4ec`.

~~~~markdown
- selector direct/indexed thresholds;
~~~~

### SRC-COMP-L1797-2D9EEE937EC0

- Kind: `context`; source: `compiler-proposal.md:1797-1797`; target: `node:CPER3`; text SHA-256: `2d9eee937ec04a3c5c73df9f404434622e4975cb3a5864f7a9202d67153faeda`.

~~~~markdown
- output/runtime/map equivalence.
~~~~

### SRC-COMP-L1799-158CAE019A6E

- Kind: `context`; source: `compiler-proposal.md:1799-1799`; target: `node:CPER3`; text SHA-256: `158cae019a6e0aeae206398211c47dc2309486847ebf015600af00666f1f3ab0`.

~~~~markdown
**Suggested predecessors:** `VCP7`, `SCP7`.
~~~~

### SRC-COMP-L1801-24C34468BB0F

- Kind: `acceptance`; source: `compiler-proposal.md:1801-1801`; target: `node:CPER3`; text SHA-256: `24c34468bb0fcb18c8ad2913d13495585a89f5d9f9101312f02b3475c0fc72b7`.

~~~~markdown
**Acceptance:** no unbounded growth, cross-framework cache collision, duplicated prerequisite work, or throughput regression hidden by parallelism; every result retains exact correctness basis.
~~~~

### SRC-COMP-L1803-8EBFC1010245

- Kind: `forbidden`; source: `compiler-proposal.md:1803-1803`; target: `node:CPER3`; text SHA-256: `8ebfc10102454b0bbcd19485cf91b5e67dc0b5d3d419f641575feb7b465186a9`.

~~~~markdown
**Forbidden:** using the soak as a global release gate or changing accepted product criteria in the join.
~~~~

### SRC-COMP-L1805-27CC19CC78E6

- Kind: `deletion`; source: `compiler-proposal.md:1805-1805`; target: `node:CPER3`; text SHA-256: `27cc19cc78e6c4a751c3dfab93465fc2a22aebacc2ff823eb060e344be66de5e`.

~~~~markdown
**Deletion/abort:** findings create bounded owner follow-ups; non-release.
~~~~

### SRC-COMP-L1807-F52D711103D5

- Kind: `context`; source: `compiler-proposal.md:1807-1807`; target: `node:CPER3`; text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`.

~~~~markdown
---
~~~~
