# Exact operative source-clause attachment — VCP5

Schema: 1. Node: `VCP5`. Clause count: 16. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-COMP-L1340-4A35BA674D42

- Kind: `context`; source: `compiler-proposal.md:1340-1340`; target: `node:VCP5`; text SHA-256: `4a35ba674d42b2b9e8dbef9ba613b5ff456c05366599016c6e2b95aa45964f7f`.

~~~~markdown
## `VCP5.md` — Vue Vapor Default compiler
~~~~

### SRC-COMP-L1342-2DE2A6B7A251

- Kind: `context`; source: `compiler-proposal.md:1342-1342`; target: `node:VCP5`; text SHA-256: `2de2a6b7a2514e59c625c27e02a7b065d1b0cd3d974682da64957b2b91d560b6`.

~~~~markdown
**Intent:** implement fine-grained Vue compilation using demanded dependency/effect relations rather than a mandatory reactivity AST.
~~~~

### SRC-COMP-L1344-E12D8166727F

- Kind: `context`; source: `compiler-proposal.md:1344-1344`; target: `node:VCP5`; text SHA-256: `e12d8166727fbc1d9fa5aa5165c3dd2800b99d5b0d08171bdeb531ad3fd2cfc3`.

~~~~markdown
**Problem:** Vapor needs richer relationships than VDOM, but a whole second reactive tree would duplicate structure and impose work on other targets.
~~~~

### SRC-COMP-L1346-56832D9ECFE1

- Kind: `context`; source: `compiler-proposal.md:1346-1346`; target: `node:VCP5`; text SHA-256: `56832d9ecfe1848d7a43f4e88a13f6f723f5dd01341c7a0613a3beb469414e2c`.

~~~~markdown
**Solution and architecture decisions:**
~~~~

### SRC-COMP-L1348-5EF1E2488FBF

- Kind: `context`; source: `compiler-proposal.md:1348-1348`; target: `node:VCP5`; text SHA-256: `5ef1e2488fbfada699194715874237f8e34b7f09c05a072b0fcdbc6a95b75a12`.

~~~~markdown
- consume canonical reactivity/read/write/dependency facts;
~~~~

### SRC-COMP-L1349-0DEBCB4EABC4

- Kind: `requirement`; source: `compiler-proposal.md:1349-1349`; target: `node:VCP5`; text SHA-256: `0debcb4eabc45551e84bfce075bd948eef19905081e8e80106042f61a43afbac`.

~~~~markdown
- build only demanded dependency sets, effect groups, ordering edges and direct-DOM operations;
~~~~

### SRC-COMP-L1350-362AC3CA0964

- Kind: `context`; source: `compiler-proposal.md:1350-1350`; target: `node:VCP5`; text SHA-256: `362ac3ca09649b146159047d70a73a63b445e0342027b38786555d8f2387ca5a`.

~~~~markdown
- index effect/operation ranges by stable Vue compiler identities;
~~~~

### SRC-COMP-L1351-F4252B717E31

- Kind: `context`; source: `compiler-proposal.md:1351-1351`; target: `node:VCP5`; text SHA-256: `f4252b717e3125ed647a27de7d575717aa1b0bc81385b356a834f06ae2c0ea49`.

~~~~markdown
- keep structure in `VCP2`, target state in sparse/graph arenas;
~~~~

### SRC-COMP-L1352-6BEE8A9D8D35

- Kind: `requirement`; source: `compiler-proposal.md:1352-1352`; target: `node:VCP5`; text SHA-256: `6bee8a9d8d35630d6eb6f432370e43df2104b157f26972ca5559edd3622dce84`.

~~~~markdown
- use only `Default` component-local evidence; project-wide evidence waits for `OPT0`;
~~~~

### SRC-COMP-L1353-05C1F2FCF800

- Kind: `context`; source: `compiler-proposal.md:1353-1353`; target: `node:VCP5`; text SHA-256: `05c1f2fcf8005734889e04baabf366f2ef129fe7df18beb91beefabaf24325fd`.

~~~~markdown
- emit through `CMP4` segmented artifacts/maps.
~~~~

### SRC-COMP-L1355-1E4BF1D3A01D

- Kind: `context`; source: `compiler-proposal.md:1355-1355`; target: `node:VCP5`; text SHA-256: `1e4bf1d3a01d79737e78ee77dc804362fd1e31beac1e93e294a66d41aa363639`.

~~~~markdown
**Suggested predecessors:** `VCP2`, `VST0`.
~~~~

### SRC-COMP-L1357-C2BCBEB3154B

- Kind: `context`; source: `compiler-proposal.md:1357-1357`; target: `node:VCP5`; text SHA-256: `c2bcbeb3154ba8b22a8dc2dd4a870135e8050c725e0d567bb46c73882ae7a7f1`.

~~~~markdown
**Suggested subblocks:** dependency graph, effect grouping, DOM operation planning, control-flow/region integration, emission/maps, conformance/performance.
~~~~

### SRC-COMP-L1359-3C49BA2B7F41

- Kind: `acceptance`; source: `compiler-proposal.md:1359-1359`; target: `node:VCP5`; text SHA-256: `3c49ba2b7f411846a63eae87aa70f7ec38723ad4617e43e1e8bb02073d89bbf9`.

~~~~markdown
**Acceptance:** no reactive AST copy exists; SSR/VDOM requests produce zero Vapor graph work; locked runtime semantics and maps pass; graph sizes/edges are ledger-visible and bounded.
~~~~

### SRC-COMP-L1361-55DFC7B3E911

- Kind: `forbidden`; source: `compiler-proposal.md:1361-1361`; target: `node:VCP5`; text SHA-256: `55dfc7b3e91135c6fe1f84b8603a8496895f8268e23f48e3cbf42db699ab8117`.

~~~~markdown
**Forbidden:** project analysis, generic proof engine, target operations stored in shared semantic facts, or production speculative candidate comparison.
~~~~

### SRC-COMP-L1363-B5A6662D06C2

- Kind: `deletion`; source: `compiler-proposal.md:1363-1363`; target: `node:VCP5`; text SHA-256: `b5a6662d06c2448d869d78b3f8f3e57348e1a6bbacebc7cb4db49fa963294ada`.

~~~~markdown
**Deletion/abort:** old Vapor path deleted at cutover only after full parity.
~~~~

### SRC-COMP-L1365-F52D711103D5

- Kind: `context`; source: `compiler-proposal.md:1365-1365`; target: `node:VCP5`; text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`.

~~~~markdown
---
~~~~
