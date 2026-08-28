# Exact operative source-clause attachment — VCP2

Schema: 1. Node: `VCP2`. Clause count: 17. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-COMP-L1201-7E571EC6AC68

- Kind: `context`; source: `compiler-proposal.md:1201-1201`; target: `node:VCP2`; text SHA-256: `7e571ec6ac6838b86c54b826cd8e00812190ffc7a34c53d5307c1565bf5314a4`.

~~~~markdown
## `VCP2.md` — Compact Vue compiler structure and canonical template topology
~~~~

### SRC-COMP-L1203-758BF0088BF8

- Kind: `context`; source: `compiler-proposal.md:1203-1203`; target: `node:VCP2`; text SHA-256: `758bf0088bf8f449405d30031e175833b7a5b2d9f69cd00787728de8bdb9b5b1`.

~~~~markdown
**Intent:** replace repeated AST relationship discovery with a compact Vue-owned structural lowering suitable for all targets.
~~~~

### SRC-COMP-L1205-5F5BEC9C8F9B

- Kind: `context`; source: `compiler-proposal.md:1205-1205`; target: `node:VCP2`; text SHA-256: `5f5bec9c8f9b043d4ceb877517fd3ac7d115b3bdbdccdbcb93efff654c92fc3d`.

~~~~markdown
**Problem:** directives/siblings/slots/control flow can be rediscovered by multiple targets, and object-heavy nodes impede cache locality.
~~~~

### SRC-COMP-L1207-56832D9ECFE1

- Kind: `context`; source: `compiler-proposal.md:1207-1207`; target: `node:VCP2`; text SHA-256: `56832d9ecfe1848d7a43f4e88a13f6f723f5dd01341c7a0613a3beb469414e2c`.

~~~~markdown
**Solution and architecture decisions:**
~~~~

### SRC-COMP-L1209-2FFEF144EEB9

- Kind: `context`; source: `compiler-proposal.md:1209-1209`; target: `node:VCP2`; text SHA-256: `2ffef144eeb967744aa2b2312cf3ce62494a5d216eb176cb5b189a64e2d8550d`.

~~~~markdown
- dense `VueCompileNodeId`, `VueTemplateNodeId`, `VueRegionId`, and ranges;
~~~~

### SRC-COMP-L1210-9562C91F8A0E

- Kind: `context`; source: `compiler-proposal.md:1210-1210`; target: `node:VCP2`; text SHA-256: `9562c91f8a0e686c64ddc1669ab513d35087e3e15a085b638e3d6225d23d4a2b`.

~~~~markdown
- region-owned `if`, `for`, slot and component-child structures;
~~~~

### SRC-COMP-L1211-8432E829F817

- Kind: `context`; source: `compiler-proposal.md:1211-1211`; target: `node:VCP2`; text SHA-256: `8432e829f81714be2a25fe2363fd02bdd061a89cd8b15628cfa4dc35c075e9fb`.

~~~~markdown
- canonical parent/child/sibling/preorder topology where demanded;
~~~~

### SRC-COMP-L1212-BD85D03E84BE

- Kind: `context`; source: `compiler-proposal.md:1212-1212`; target: `node:VCP2`; text SHA-256: `bd85d03e84be55614ae976a7bec4fbdb1ecda0daf25a324e11f96d4f8aa96c56`.

~~~~markdown
- source spans/anchors, semantic references and target decisions in side tables;
~~~~

### SRC-COMP-L1213-1A778A9F14AD

- Kind: `context`; source: `compiler-proposal.md:1213-1213`; target: `node:VCP2`; text SHA-256: `1a778a9f14ad192ca9dc64ad621318c4e79fd8150a2fc642f6b2dd2783dd31fc`.

~~~~markdown
- flat attribute/child/directive arenas and interned names;
~~~~

### SRC-COMP-L1214-D2E239D7527A

- Kind: `context`; source: `compiler-proposal.md:1214-1214`; target: `node:VCP2`; text SHA-256: `d2e239d7527ac46b7b30d4e8831e6fd9dbd954161045f19a33eb0235f4a5fd12`.

~~~~markdown
- logical materialization contract with future streaming permission;
~~~~

### SRC-COMP-L1215-2B308EFCEE2C

- Kind: `context`; source: `compiler-proposal.md:1215-1215`; target: `node:VCP2`; text SHA-256: `2b308efcee2c37cf86a99b9307282abbc7db09515e3a25adf653645f7d72d908`.

~~~~markdown
- no target-specific patch/effect/server state in structural nodes.
~~~~

### SRC-COMP-L1217-21BEDA004DA0

- Kind: `context`; source: `compiler-proposal.md:1217-1217`; target: `node:VCP2`; text SHA-256: `21beda004da0eceadc30037990e9e697609cce20fa9ab17c7ac98c89da679849`.

~~~~markdown
**Suggested predecessor:** `VCP1`.
~~~~

### SRC-COMP-L1219-5364A0A95826

- Kind: `context`; source: `compiler-proposal.md:1219-1219`; target: `node:VCP2`; text SHA-256: `5364a0a95826c3986513fdf103f9b7348e6c749d339f8b1d36ac7240a85e8506`.

~~~~markdown
**Suggested subblocks:** ID/arena migration, control-flow regions, slot/component regions, topology, side-table/data-layout conversion, dumps/verifiers.
~~~~

### SRC-COMP-L1221-5A4EE4C0AF33

- Kind: `forbidden`; source: `compiler-proposal.md:1221-1221`; target: `node:VCP2`; text SHA-256: `5a4ee4c0af3315bc780296ddac34028cf667bd5e013580729bed11b6cd71f3d2`.

~~~~markdown
**Acceptance:** all targets can consume one structural authority; node access is O(1) by dense ID; source offsets remain separate; node-size/allocation budgets pass; malformed source never enters admitted lowering.
~~~~

### SRC-COMP-L1223-C56EA55A1322

- Kind: `forbidden`; source: `compiler-proposal.md:1223-1223`; target: `node:VCP2`; text SHA-256: `c56ea55a1322bb7a8417820db3338eae0f0f6060f27fdd0b00e9c8f388c6c620`.

~~~~markdown
**Forbidden:** source-offset node IDs, target flags in structural nodes, per-node `Vec`/`String` defaults, or universal UI operations.
~~~~

### SRC-COMP-L1225-DE64B8D120CC

- Kind: `deletion`; source: `compiler-proposal.md:1225-1225`; target: `node:VCP2`; text SHA-256: `de64b8d120cc20c62e4314051e9bb3a825d4d21900f642d29b1c03102705924f`.

~~~~markdown
**Deletion/abort:** migrate behavior-preservingly and delete old shared walkers only when their final target moves.
~~~~

### SRC-COMP-L1227-F52D711103D5

- Kind: `context`; source: `compiler-proposal.md:1227-1227`; target: `node:VCP2`; text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`.

~~~~markdown
---
~~~~
