# Exact operative source-clause attachment — VST1

Schema: 1. Node: `VST1`. Clause count: 19. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-COMP-L1257-56B783CE3304

- Kind: `context`; source: `compiler-proposal.md:1257-1257`; target: `node:VST1`; text SHA-256: `56b783ce33044714c35e5015d54cee652504001d47a3c54dc968841d70db426b`.

~~~~markdown
## `VST1.md` — Vue selector-to-template query engine
~~~~

### SRC-COMP-L1259-E2BAF1185E45

- Kind: `context`; source: `compiler-proposal.md:1259-1259`; target: `node:VST1`; text SHA-256: `e2baf1185e45062f53149ec3dee05c56e5a8cf08f4859cd1537cc0aa85838b67`.

~~~~markdown
**Intent:** provide a Vue-owned selector applicability service for tooling and future optimization without taxing default runtime compilation.
~~~~

### SRC-COMP-L1261-DC6EA449CCEA

- Kind: `context`; source: `compiler-proposal.md:1261-1261`; target: `node:VST1`; text SHA-256: `dc6ea449ccea3dbd871485832faec638fd7d135b2237c673d20e162fc010f85c`.

~~~~markdown
**Problem:** CSS diagnostics/navigation/component analysis need selector-to-template relationships, but Vue default runtime compilation does not require selector pruning and should not pay for it.
~~~~

### SRC-COMP-L1263-56832D9ECFE1

- Kind: `context`; source: `compiler-proposal.md:1263-1263`; target: `node:VST1`; text SHA-256: `56832d9ecfe1848d7a43f4e88a13f6f723f5dd01341c7a0613a3beb469414e2c`.

~~~~markdown
**Solution and architecture decisions:**
~~~~

### SRC-COMP-L1265-9ABF6D07A24B

- Kind: `context`; source: `compiler-proposal.md:1265-1265`; target: `node:VST1`; text SHA-256: `9abf6d07a24b81f956dd24ae0e93a1892a5d4434686addb2bb9ad781eae1d74e`.

~~~~markdown
- consume J selector structure and `VCP2` Vue template topology;
~~~~

### SRC-COMP-L1266-0E1AF0410204

- Kind: `requirement`; source: `compiler-proposal.md:1266-1266`; target: `node:VST1`; text SHA-256: `0e1af04102044764b3f75b1051e003c1373723dff83987cb39f46b20a205d8b8`.

~~~~markdown
- derive a compact selector query plan only when demanded and cost-effective;
~~~~

### SRC-COMP-L1267-CC88520214F5

- Kind: `context`; source: `compiler-proposal.md:1267-1267`; target: `node:VST1`; text SHA-256: `cc88520214f5c5527d70f864c390060cc8df7fab97ea9c3b6907cea91e955751`.

~~~~markdown
- use adaptive direct versus indexed matching;
~~~~

### SRC-COMP-L1268-124DCB4EFEF0

- Kind: `forbidden`; source: `compiler-proposal.md:1268-1268`; target: `node:VST1`; text SHA-256: `124dcb4efef0231af66477f11ab93de9c0c91801e1c4f3c11e2602a4cbdff0a1`.

~~~~markdown
- postings use only sound positive anchors; negated predicates never seed candidates;
~~~~

### SRC-COMP-L1269-89F5A1554016

- Kind: `context`; source: `compiler-proposal.md:1269-1269`; target: `node:VST1`; text SHA-256: `89f5a1554016b6a30042364a7722361a74b2fe069593a1c36d6d84a7d86e0686`.

~~~~markdown
- dynamic tags/classes/IDs/attributes and spreads enter explicit maybe buckets;
~~~~

### SRC-COMP-L1270-9AC095E12DB2

- Kind: `requirement`; source: `compiler-proposal.md:1270-1270`; target: `node:VST1`; text SHA-256: `9ac095e12db278889658492d1abe0a6c93e492b29ebe7ee2f48931352f1c7548`.

~~~~markdown
- exact Vue matcher returns `Yes | Maybe | No` and remains authoritative;
~~~~

### SRC-COMP-L1271-133978099F24

- Kind: `context`; source: `compiler-proposal.md:1271-1271`; target: `node:VST1`; text SHA-256: `133978099f2467000d49bcf2679ccfbf9b7afecd1b6e2ad4d4d513c211b68507`.

~~~~markdown
- produce `VueStyleMatchFacts` for diagnostics, navigation, component information and future `Optimized` consideration;
~~~~

### SRC-COMP-L1272-5181DE322672

- Kind: `requirement`; source: `compiler-proposal.md:1272-1272`; target: `node:VST1`; text SHA-256: `5181de322672736ccf8913d6f0d8fba2027d99bdd0beeb7c3b7519c720b5adae`.

~~~~markdown
- `Default` runtime targets demand none of this work unless a separately locked correctness cell requires it;
~~~~

### SRC-COMP-L1273-5AE5FD71083C

- Kind: `context`; source: `compiler-proposal.md:1273-1273`; target: `node:VST1`; text SHA-256: `5ae5fd71083c194a485e6a710e31e83ce155d73227a0fcd91665374d4d8e572b`.

~~~~markdown
- no pruning behavior is admitted by this block.
~~~~

### SRC-COMP-L1275-1E4BF1D3A01D

- Kind: `context`; source: `compiler-proposal.md:1275-1275`; target: `node:VST1`; text SHA-256: `1e4bf1d3a01d79737e78ee77dc804362fd1e31beac1e93e294a66d41aa363639`.

~~~~markdown
**Suggested predecessors:** `VCP2`, `VST0`.
~~~~

### SRC-COMP-L1277-4F6DD79FCF20

- Kind: `context`; source: `compiler-proposal.md:1277-1277`; target: `node:VST1`; text SHA-256: `4f6dd79fcf2094b43f4a2c89c25ef904aa21c9e1225117b280d440188709e80f`.

~~~~markdown
**Suggested subblocks:** semantic contract, direct matcher, topology feature index, selector query plan, adaptive cost model, fact/witness publication and performance gates.
~~~~

### SRC-COMP-L1279-06DE28D11A43

- Kind: `acceptance`; source: `compiler-proposal.md:1279-1279`; target: `node:VST1`; text SHA-256: `06de28d11a43ea435714d5040d734cc83b4cb67243a9253c3decadda9c8e95a2`.

~~~~markdown
**Acceptance:** direct and indexed paths are semantically identical; candidate reduction has no false negatives; dynamic cases remain `Maybe`; default compiler ledgers show zero VST1 work; tooling consumers can request sparse witnesses without production overhead.
~~~~

### SRC-COMP-L1281-F7FE3CF656C7

- Kind: `forbidden`; source: `compiler-proposal.md:1281-1281`; target: `node:VST1`; text SHA-256: `f7fe3cf656c7e4ebac875ea79a18c24f6bd26f01e412791f8f3f879ca561734d`.

~~~~markdown
**Forbidden:** making VST1 a VCP7 predecessor, universal selector semantics, always building an index, or using `Maybe` to remove CSS.
~~~~

### SRC-COMP-L1283-78EB7F1FFCFF

- Kind: `deletion`; source: `compiler-proposal.md:1283-1283`; target: `node:VST1`; text SHA-256: `78eb7f1ffcff2bfce4e2b359eb042bf2f0043aaea474101169ae7694139d3606`.

~~~~markdown
**Deletion/abort:** no runtime compiler deletion; move shared mechanics only after measured neutral equivalence.
~~~~

### SRC-COMP-L1285-F52D711103D5

- Kind: `context`; source: `compiler-proposal.md:1285-1285`; target: `node:VST1`; text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`.

~~~~markdown
---
~~~~
