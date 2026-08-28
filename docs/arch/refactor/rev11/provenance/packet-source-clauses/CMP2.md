# Exact operative source-clause attachment — CMP2

Schema: 1. Node: `CMP2`. Clause count: 19. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-COMP-L1000-9D129C962A88

- Kind: `context`; source: `compiler-proposal.md:1000-1000`; target: `node:CMP2`; text SHA-256: `9d129c962a88098467c4a59c66fcd44a9368bbadb3d77cf8d5a833914a5d7f52`.

~~~~markdown
- child, attribute, operation, dependency and relation collections use flat arenas plus ranges;
~~~~

### SRC-COMP-L1001-1B6067B6ADD4

- Kind: `requirement`; source: `compiler-proposal.md:1001-1001`; target: `node:CMP2`; text SHA-256: `1b6067b6add49fbdebaf8e8b13d182bcacaf9b5d00abeea312cf2e3bd982c69d`.

~~~~markdown
- raw authored slices remain source-backed; only requested decoded/interned/normalized values allocate;
~~~~

### SRC-COMP-L1002-B74331FB40EC

- Kind: `requirement`; source: `compiler-proposal.md:1002-1002`; target: `node:CMP2`; text SHA-256: `b74331fb40ec91da435a7c6a322386758d3a6dd7efe319cf634ba8fa5cf10d6b`.

~~~~markdown
- lifetime classes are explicit (`Frontend`, `Semantic`, `CompilerScratch`, `TargetScratch`, `Emission`) and may be combined only through measurement;
~~~~

### SRC-COMP-L1003-AA1B12CC6F0E

- Kind: `context`; source: `compiler-proposal.md:1003-1003`; target: `node:CMP2`; text SHA-256: `aa1b12cc6f0ee729069656948cccdd1d5688e3e78b6103fb003cdd61a79d1231`.

~~~~markdown
- canonical compiler structures are logical contracts; direct one-shot execution may later stream/fuse portions after materialized parity is proven.
~~~~

### SRC-COMP-L1005-B328913439D5

- Kind: `context`; source: `compiler-proposal.md:1005-1005`; target: `node:CMP2`; text SHA-256: `b328913439d5f95bd18dca6cf16fabc8838d93b87db4bf93b40e23a6e5ab2f0d`.

~~~~markdown
**Suggested predecessor:** `CMP1`.
~~~~

### SRC-COMP-L1007-4FD2E85B260D

- Kind: `context`; source: `compiler-proposal.md:1007-1007`; target: `node:CMP2`; text SHA-256: `4fd2e85b260dfddccd394978f7159771397af9338475fd3a6d47a2f37e61558a`.

~~~~markdown
**Suggested subblocks:** typed ID/arena primitives, span/offset indexes, region storage, topology storage, interning/range migration, lifetime/size verification.
~~~~

### SRC-COMP-L1009-D1C878B8D2B8

- Kind: `acceptance`; source: `compiler-proposal.md:1009-1009`; target: `node:CMP2`; text SHA-256: `d1c878b8d2b86639adcd8bc48bf1ba8002468e871dbc36cce4fce5153f5eeb95`.

~~~~markdown
**Acceptance:** `nodes[id.index()]` is O(1) with compact dense storage; source-position lookup remains exact through a separate index; no source-length-sized sparse node arena is required; node-size and bytes/node gates pass; no hot node owns variable-size collections directly unless a measured exception is ratified.
~~~~

### SRC-COMP-L1011-26777BD1AC40

- Kind: `forbidden`; source: `compiler-proposal.md:1011-1011`; target: `node:CMP2`; text SHA-256: `26777bd1ac40eef9d87493eb7e0097e83092eea47edbb0d748f2259fc3c4a5a5`.

~~~~markdown
**Forbidden:** `NodeId = authored byte offset`, cross-revision offset identity, one arena by ideology, universal semantic node kinds, or copied source strings for ownership.
~~~~

### SRC-COMP-L1013-5537C0840A44

- Kind: `deletion`; source: `compiler-proposal.md:1013-1013`; target: `node:CMP2`; text SHA-256: `5537c0840a4447f0cfa0e48ea4b1f6cec67f014f0d2959c39d0183e51589d138`.

~~~~markdown
**Deletion/abort:** migrate one framework structure at a time; abort any “shared” node/region primitive that requires framework branches.
~~~~

### SRC-COMP-L1015-F52D711103D5

- Kind: `context`; source: `compiler-proposal.md:1015-1015`; target: `node:CMP2`; text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`.

~~~~markdown
---
~~~~

### SRC-COMP-L987-4C86D312BC41

- Kind: `context`; source: `compiler-proposal.md:987-987`; target: `node:CMP2`; text SHA-256: `4c86d312bc419898cc365c3d8a1733430f87677d3647cba0801b1dc16dca16e1`.

~~~~markdown
## `CMP2.md` — Data-oriented compiler structure, regions, topology, and lifetime model
~~~~

### SRC-COMP-L989-898B17B84A6F

- Kind: `context`; source: `compiler-proposal.md:989-989`; target: `node:CMP2`; text SHA-256: `898b17b84a6fa6b0da98534837a3233c1c28173c6d27fd9b4840af7fc9ab8c8a`.

~~~~markdown
**Intent:** establish compact framework-neutral mechanics while preserving framework-native compiler meaning.
~~~~

### SRC-COMP-L991-872394D7CBE7

- Kind: `context`; source: `compiler-proposal.md:991-991`; target: `node:CMP2`; text SHA-256: `872394d7cbe7da42f544798df23496b095a7f512c02be3574669a9aeab63db3b`.

~~~~markdown
**Problem:** object graphs with per-node `String`, `Vec`, `HashMap`, copied text, and source-offset identities increase allocation/RSS and make repeated structural discovery likely.
~~~~

### SRC-COMP-L993-56832D9ECFE1

- Kind: `context`; source: `compiler-proposal.md:993-993`; target: `node:CMP2`; text SHA-256: `56832d9ecfe1848d7a43f4e88a13f6f723f5dd01341c7a0613a3beb469414e2c`.

~~~~markdown
**Solution and architecture decisions:**
~~~~

### SRC-COMP-L995-67A725048F67

- Kind: `context`; source: `compiler-proposal.md:995-995`; target: `node:CMP2`; text SHA-256: `67a725048f67a906a55fbe518ef39c12b3708a8e7e07ea7224d7455dec37b450`.

~~~~markdown
- dense snapshot-local typed IDs are direct arena indices;
~~~~

### SRC-COMP-L996-36AF05C90D42

- Kind: `forbidden`; source: `compiler-proposal.md:996-996`; target: `node:CMP2`; text SHA-256: `36af05c90d42602ae63f30db428af58796342a4682aab76a1400189ad64a0d90`.

~~~~markdown
- authored start/end offsets live in side tables and never define compiler identity;
~~~~

### SRC-COMP-L997-BA76AB843CC1

- Kind: `context`; source: `compiler-proposal.md:997-997`; target: `node:CMP2`; text SHA-256: `ba76ab843cc18124832023e030a7c60dff238f0ad1884bcdcd0fa45d446ce81b`.

~~~~markdown
- region-owned control flow normalizes branch/body ownership once;
~~~~

### SRC-COMP-L998-9A54DC9328FD

- Kind: `context`; source: `compiler-proposal.md:998-998`; target: `node:CMP2`; text SHA-256: `9a54dc9328fdaff2dfb24f6dd773bfa57ac3d66d8da30ef1b6b51d4d81483c62`.

~~~~markdown
- compact topology tables provide parent/child/sibling/preorder/region relations where a framework demands them;
~~~~

### SRC-COMP-L999-263EF8A24B59

- Kind: `context`; source: `compiler-proposal.md:999-999`; target: `node:CMP2`; text SHA-256: `263ef8a24b59d26346e549a545d2f162736db9161d57398ea217ee7547af6e99`.

~~~~markdown
- hot classifications use packed/dense tables; rare facts use sparse tables;
~~~~
