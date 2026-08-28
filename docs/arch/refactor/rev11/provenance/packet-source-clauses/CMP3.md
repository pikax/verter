# Exact operative source-clause attachment — CMP3

Schema: 1. Node: `CMP3`. Clause count: 21. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-COMP-L1017-1C672CE8C07B

- Kind: `context`; source: `compiler-proposal.md:1017-1017`; target: `node:CMP3`; text SHA-256: `1c672ce8c07b56b47d0dbd5d9e64d5454b997c6ecbebd6fa797cb3443a3abb19`.

~~~~markdown
## `CMP3.md` — Framework-native target planning and static physical execution
~~~~

### SRC-COMP-L1019-FA31B5D5CFE5

- Kind: `requirement`; source: `compiler-proposal.md:1019-1019`; target: `node:CMP3`; text SHA-256: `fa31b5d5cfe50836f0c179b605d14017733a1f77112bfb0399a5fc207a3ff6de`.

~~~~markdown
**Intent:** compile only the relationships required by each requested target without universal lowering or dynamic pass dispatch.
~~~~

### SRC-COMP-L1021-2B7148213510

- Kind: `context`; source: `compiler-proposal.md:1021-1021`; target: `node:CMP3`; text SHA-256: `2b7148213510249e08cf83d2e601bc213e8f0249db8fc8c799758ada1995f74e`.

~~~~markdown
**Problem:** whole target-tree copies, mandatory reactivity IRs, runtime pass registries, and per-node strategy calls waste work and leak framework semantics.
~~~~

### SRC-COMP-L1023-56832D9ECFE1

- Kind: `context`; source: `compiler-proposal.md:1023-1023`; target: `node:CMP3`; text SHA-256: `56832d9ecfe1848d7a43f4e88a13f6f723f5dd01341c7a0613a3beb469414e2c`.

~~~~markdown
**Solution and architecture decisions:**
~~~~

### SRC-COMP-L1025-FFD98B4BB3E5

- Kind: `context`; source: `compiler-proposal.md:1025-1025`; target: `node:CMP3`; text SHA-256: `ffd98b4bb3e59ae4a2e4c29c2312a4794a6d076b1b292ab5ae2995dc9c185bf8`.

~~~~markdown
- each framework owns a private compiler structure and target executors;
~~~~

### SRC-COMP-L1026-14BF2C6233BC

- Kind: `context`; source: `compiler-proposal.md:1026-1026`; target: `node:CMP3`; text SHA-256: `14bf2c6233bc9f0d8c920e6be187480440aae6c8755a7806c9dece5488afb0bc`.

~~~~markdown
- framework selection and target selection occur once outside hot loops;
~~~~

### SRC-COMP-L1027-F67FDC55C56F

- Kind: `context`; source: `compiler-proposal.md:1027-1027`; target: `node:CMP3`; text SHA-256: `f67fdc55c56f2de49975424e0fe4d253284103a709698ea995c835e609bfa4ec`.

~~~~markdown
- logical operations classify as local synthesized, regional, barrier graph, target planning, emission, or terminal materialization;
~~~~

### SRC-COMP-L1028-8CBCB8D4E77D

- Kind: `context`; source: `compiler-proposal.md:1028-1028`; target: `node:CMP3`; text SHA-256: `8cbcb8d4e77d0b1c884146ca7f5641560990988aaf83d04247b23328f889269c`.

~~~~markdown
- local facts fuse into existing typed visits;
~~~~

### SRC-COMP-L1029-A70E4B406360

- Kind: `context`; source: `compiler-proposal.md:1029-1029`; target: `node:CMP3`; text SHA-256: `a70e4b406360f2b1e3d63d2cd1b752e264be0fe7fbdd99fcfea792d2538cf88c`.

~~~~markdown
- barrier algorithms operate on compact tables/graphs, not the syntax tree;
~~~~

### SRC-COMP-L1030-FC8964ACAB56

- Kind: `context`; source: `compiler-proposal.md:1030-1030`; target: `node:CMP3`; text SHA-256: `fc8964acab56f0c06560d6a82025cb49e13d33b951e5e4e58e030e787d57b4db`.

~~~~markdown
- VDOM-like targets use sparse patch/hoist/cache overlays;
~~~~

### SRC-COMP-L1031-A492E96E7136

- Kind: `context`; source: `compiler-proposal.md:1031-1031`; target: `node:CMP3`; text SHA-256: `a492e96e713661f7b761b5cb61e93d94527eb0249284106ac288d7ce79583d1a`.

~~~~markdown
- fine-grained client targets request compact dependency/effect/operation graphs;
~~~~

### SRC-COMP-L1032-1BDBA08D304A

- Kind: `context`; source: `compiler-proposal.md:1032-1032`; target: `node:CMP3`; text SHA-256: `1bdba08d304a602e45db18465fa13a94e535de142e5209c9f36eb6d990bfbf85`.

~~~~markdown
- server targets request no client effect graph;
~~~~

### SRC-COMP-L1033-59EE5128007B

- Kind: `requirement`; source: `compiler-proposal.md:1033-1033`; target: `node:CMP3`; text SHA-256: `59ee5128007bab11aecf8f67e75fc5e03e2e6cce2b1f60e92eccebc46a6139c8`.

~~~~markdown
- target structure is materialized only when it avoids rediscovery, enables reuse, or is required by a barrier;
~~~~

### SRC-COMP-L1034-25CDB419D6D2

- Kind: `context`; source: `compiler-proposal.md:1034-1034`; target: `node:CMP3`; text SHA-256: `25cdb419d6d225198ec95472fc6c2dec62ef6f3852b71898fd40dc62405b985a`.

~~~~markdown
- compatible multi-target requests share parse, semantic and structural prerequisites and branch at the minimum target-specific point;
~~~~

### SRC-COMP-L1035-3BC51974D98D

- Kind: `context`; source: `compiler-proposal.md:1035-1035`; target: `node:CMP3`; text SHA-256: `3bc51974d98db64a3d2bfd4424273f200b61cd5e716d0b85875d42f2adf1e87f`.

~~~~markdown
- shared semantic abstractions follow a rule of three; two similarly named framework constructs are insufficient.
~~~~

### SRC-COMP-L1037-DC6858138762

- Kind: `context`; source: `compiler-proposal.md:1037-1037`; target: `node:CMP3`; text SHA-256: `dc6858138762fe11084c5311c16b15958eaf836d01668f86a4c197d346b26500`.

~~~~markdown
**Suggested predecessor:** `CMP2`.
~~~~

### SRC-COMP-L1039-9161B6D4715E

- Kind: `deletion`; source: `compiler-proposal.md:1039-1039`; target: `node:CMP3`; text SHA-256: `9161b6d4715eba2c3cb99bd7c567bb7500d5d7ff40c750f2e1d18232cdf7fbcb`.

~~~~markdown
**Suggested subblocks:** execution classes, static target executor pattern, sparse overlay primitives, dependency/effect graph primitives, multi-target branch planner, dynamic-dispatch deletion guards.
~~~~

### SRC-COMP-L1041-8F61BEF001B8

- Kind: `acceptance`; source: `compiler-proposal.md:1041-1041`; target: `node:CMP3`; text SHA-256: `8f61bef001b8c4826d2dc87217f6757f29680ce6b9b6a2308b36154d14e23b3b`.

~~~~markdown
**Acceptance:** no accepted hot loop uses per-node dynamic target dispatch; server-only targets produce zero effect-plan ledger entries; target overlays contain only target-specific state; multi-target requests prove shared prerequisites; a synthetic second framework can use the mechanics without importing the first framework’s semantics.
~~~~

### SRC-COMP-L1043-39FCC97FB525

- Kind: `forbidden`; source: `compiler-proposal.md:1043-1043`; target: `node:CMP3`; text SHA-256: `39fcc97fb52598cfe3f7f432b4ad3d4b896d1f2b7796b00d2963ccee9eaf51ed`.

~~~~markdown
**Forbidden:** universal UI IR, mandatory reactive AST, runtime plugin pass graph, full target tree for symmetry, or speculative build-two-and-discard-one production optimization.
~~~~

### SRC-COMP-L1045-79FCB3B4C057

- Kind: `deletion`; source: `compiler-proposal.md:1045-1045`; target: `node:CMP3`; text SHA-256: `79fcb3b4c057f6b249bb26dbd3bf3757e76c4115e48ca957e752285818d09707`.

~~~~markdown
**Deletion/abort:** delete old strategy/walker dispatch only after target parity; move any framework-shaped shared abstraction back to its owner.
~~~~

### SRC-COMP-L1047-F52D711103D5

- Kind: `context`; source: `compiler-proposal.md:1047-1047`; target: `node:CMP3`; text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`.

~~~~markdown
---
~~~~
