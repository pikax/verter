# Exact operative source-clause attachment — SST2

Schema: 1. Node: `SST2`. Clause count: 18. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-COMP-L1587-2560056182E3

- Kind: `context`; source: `compiler-proposal.md:1587-1587`; target: `node:SST2`; text SHA-256: `2560056182e3e46c38c567056c402e702bc2463a4124c1bec430368fdc7cf841`.

~~~~markdown
## `SST2.md` — Svelte style-match facts and adaptive matcher cutover
~~~~

### SRC-COMP-L1589-88F961DB7929

- Kind: `deletion`; source: `compiler-proposal.md:1589-1589`; target: `node:SST2`; text SHA-256: `88f961db79296002cb43b02d9246dd7bc81f71b288dc19bca54ba9b8c485a932`.

~~~~markdown
**Intent:** publish selector applicability/scoping/pruning facts once for compiler, lint, IDE and metadata and delete compiler-local matcher ownership.
~~~~

### SRC-COMP-L1591-2F711EC80F64

- Kind: `context`; source: `compiler-proposal.md:1591-1591`; target: `node:SST2`; text SHA-256: `2f711ec80f641afd3d3ab76a5766b8a94201fb5e2ceb935bc81ce9f48077d43a`.

~~~~markdown
**Problem:** multiple consumers can repeat matching or retain heavy witness/path data; uncertain selectors can be pruned unsafely.
~~~~

### SRC-COMP-L1593-56832D9ECFE1

- Kind: `context`; source: `compiler-proposal.md:1593-1593`; target: `node:SST2`; text SHA-256: `56832d9ecfe1848d7a43f4e88a13f6f723f5dd01341c7a0613a3beb469414e2c`.

~~~~markdown
**Solution and architecture decisions:**
~~~~

### SRC-COMP-L1595-37D6CA828AD7

- Kind: `context`; source: `compiler-proposal.md:1595-1595`; target: `node:SST2`; text SHA-256: `37d6ca828ad770ab7f910e55450db6b942b27b306967dc20c901c02076e490f0`.

~~~~markdown
- produce compact `SvelteStyleMatchFacts`:
~~~~

### SRC-COMP-L1597-1F5400E380F8

- Kind: `context`; source: `compiler-proposal.md:1597-1603`; target: `node:SST2`; text SHA-256: `1f5400e380f8d67153f12845f91e14c55c236381dee391387d24a108782100ca`.

~~~~markdown
```text
  selector_use: Yes | Maybe | No
  scoped_template_nodes: dense bitset
  scoped_selector_compounds: dense bitset
  uncertainty reasons: sparse
  witnesses: optional sparse arena
  ```
~~~~

### SRC-COMP-L1605-C9E29AED7F06

- Kind: `context`; source: `compiler-proposal.md:1605-1605`; target: `node:SST2`; text SHA-256: `c9e29aed7f061122a8a6ebe530604791658191a34c91e8cf71b7640d52a05c0e`.

~~~~markdown
- choose direct/indexed strategy once per component with the locked cost model;
~~~~

### SRC-COMP-L1606-CA74E867AE8A

- Kind: `requirement`; source: `compiler-proposal.md:1606-1606`; target: `node:SST2`; text SHA-256: `ca74e867ae8afbeff94b6465e968bcbd376a286ff13672a855399e3fa1f23391`.

~~~~markdown
- exact verifier walks complete selector semantics right-to-left;
~~~~

### SRC-COMP-L1607-C3D7E53FB3D7

- Kind: `requirement`; source: `compiler-proposal.md:1607-1607`; target: `node:SST2`; text SHA-256: `c3d7e53fb3d761cce415648471952e4458a79fed4d4152e7207050c6cb31a6dd`.

~~~~markdown
- only `No` permits pruning;
~~~~

### SRC-COMP-L1608-D64842911428

- Kind: `context`; source: `compiler-proposal.md:1608-1608`; target: `node:SST2`; text SHA-256: `d64842911428fcc7e0eee59852e116970e4b650f81787e2f2888b6edbf8131e3`.

~~~~markdown
- `PruneOnly`, `ScopePlan`, `Diagnostics`, and `ConformanceTrace` demand products materialize different data;
~~~~

### SRC-COMP-L1609-8D796A60C401

- Kind: `context`; source: `compiler-proposal.md:1609-1609`; target: `node:SST2`; text SHA-256: `8d796a60c4019b12b09808fc83a1d7344e91c4320e8ba979bee887ef2d787b91`.

~~~~markdown
- client and server requested together reuse one style-match product;
~~~~

### SRC-COMP-L1610-19C76A6AD01E

- Kind: `context`; source: `compiler-proposal.md:1610-1610`; target: `node:SST2`; text SHA-256: `19c76a6ad01e641c805db8d5e80b413f0c937ec67b03cf2f7e8cb7136a3cdb3d`.

~~~~markdown
- detailed witnesses are absent from production compile unless demanded.
~~~~

### SRC-COMP-L1612-5F987FE04C82

- Kind: `context`; source: `compiler-proposal.md:1612-1612`; target: `node:SST2`; text SHA-256: `5f987fe04c82fa2737016d0f72f8caaf1e38cf610e6f7593d616f8a3c614f945`.

~~~~markdown
**Suggested predecessor:** `SST1`.
~~~~

### SRC-COMP-L1614-0DD3E74C838F

- Kind: `deletion`; source: `compiler-proposal.md:1614-1614`; target: `node:SST2`; text SHA-256: `0dd3e74c838fd6d5afad21a740d09ed2fa819461e031d38717ccc084687a28fc`.

~~~~markdown
**Suggested subblocks:** fact schema, exact verifier integration, scope/prune products, diagnostic witnesses, consumer cutover, old matcher/index deletion and performance terminal.
~~~~

### SRC-COMP-L1616-5FAD3B4C80C0

- Kind: `deletion`; source: `compiler-proposal.md:1616-1616`; target: `node:SST2`; text SHA-256: `5fad3b4c80c01b6efdc730698daf0efc13bbfc639951b5df7be7cdc22f06825f`.

~~~~markdown
**Acceptance:** no pruning false negatives across the locked corpus; `Maybe` always fails open; client/server/lint/IDE share one fact basis; `PruneOnly` materializes zero witnesses; old runtime-IR matcher authority is deleted.
~~~~

### SRC-COMP-L1618-DF36815A2261

- Kind: `forbidden`; source: `compiler-proposal.md:1618-1618`; target: `node:SST2`; text SHA-256: `df36815a22612f61d6460dd148dfd4a5d16b33f2489c06cdc20550243f776878`.

~~~~markdown
**Forbidden:** `Maybe` pruning, target-specific repeated matching, witness strings in dense facts, or hidden full element scans in indexed mode.
~~~~

### SRC-COMP-L1620-A67AEC9CB335

- Kind: `deletion`; source: `compiler-proposal.md:1620-1620`; target: `node:SST2`; text SHA-256: `a67aec9cb335e8a19a846cce5fad4c3564a4ce29600e8d4b5b7a57ba83cf66bb`.

~~~~markdown
**Deletion/abort:** this is the sole Svelte matcher cutover/deletion owner; revert to direct exact matching rather than weaken correctness.
~~~~

### SRC-COMP-L1622-F52D711103D5

- Kind: `context`; source: `compiler-proposal.md:1622-1622`; target: `node:SST2`; text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`.

~~~~markdown
---
~~~~
