# Exact operative source-clause attachment — SCP2

Schema: 1. Node: `SCP2`. Clause count: 18. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-COMP-L1480-C6525ABC0FCE

- Kind: `context`; source: `compiler-proposal.md:1480-1480`; target: `node:SCP2`; text SHA-256: `c6525abc0fce13593e79d674a19b310c90b53b1f7d1ba541fcda93fcd59977ce`.

~~~~markdown
## `SCP2.md` — Compact Svelte compiler structure and canonical template topology
~~~~

### SRC-COMP-L1482-0F72EB7F9387

- Kind: `context`; source: `compiler-proposal.md:1482-1482`; target: `node:SCP2`; text SHA-256: `0f72eb7f93873f72bb98c7844e59e002e966c2e5992afb79dc49e548f505d289`.

~~~~markdown
**Intent:** build one source-authoritative Svelte structural/topology product before target lowering erases information.
~~~~

### SRC-COMP-L1484-3C9334A82ED7

- Kind: `context`; source: `compiler-proposal.md:1484-1484`; target: `node:SCP2`; text SHA-256: `3c9334a82ed7b7b3bf3e9c947fd5de586b58bba11d81be04ad49ab722eb8847a`.

~~~~markdown
**Problem:** style matching and target transforms can reconstruct paths from runtime IR, while object-heavy nodes retain repeated strings/vectors and target concerns.
~~~~

### SRC-COMP-L1486-56832D9ECFE1

- Kind: `context`; source: `compiler-proposal.md:1486-1486`; target: `node:SCP2`; text SHA-256: `56832d9ecfe1848d7a43f4e88a13f6f723f5dd01341c7a0613a3beb469414e2c`.

~~~~markdown
**Solution and architecture decisions:**
~~~~

### SRC-COMP-L1488-9FF238255BD3

- Kind: `context`; source: `compiler-proposal.md:1488-1488`; target: `node:SCP2`; text SHA-256: `9ff238255bd3137a8f98f2cad64df94eb9b87d24eed5d59066bccf4fcb803172`.

~~~~markdown
- dense Svelte-owned node/region/expression/scope IDs;
~~~~

### SRC-COMP-L1489-F8DC534F26F1

- Kind: `context`; source: `compiler-proposal.md:1489-1489`; target: `node:SCP2`; text SHA-256: `f8dc534f26f1327a3b967595640771c7e684e8b40e80e39279a67fae406e7609`.

~~~~markdown
- region-owned `if`, `each`, `await`, `key`, snippet and slot/component structures;
~~~~

### SRC-COMP-L1490-434DD32BC9BB

- Kind: `context`; source: `compiler-proposal.md:1490-1490`; target: `node:SCP2`; text SHA-256: `434dd32bc9bbd13559f2e82caa6d23a2c5800ad33d2b3ea22f9facb28f2750d5`.

~~~~markdown
- canonical topology side tables:
~~~~

### SRC-COMP-L1492-F4F9A3A2E686

- Kind: `context`; source: `compiler-proposal.md:1492-1501`; target: `node:SCP2`; text SHA-256: `f4f9a3a2e686750f7815cabcf9e728cf2377508de74b409fbd706a95fb7041bd`.

~~~~markdown
```text
  parent
  first_child
  next_sibling
  previous_sibling
  preorder_start/end
  region/existence class
  static/dynamic tag/id/class/attribute facts
  snippet definition/render-site edges
  ```
~~~~

### SRC-COMP-L1503-0A5DB4BF6881

- Kind: `context`; source: `compiler-proposal.md:1503-1503`; target: `node:SCP2`; text SHA-256: `0a5db4bf688152a6ca892b13de458d43600985a3f258e81f559aaa6962bdfffe`.

~~~~markdown
- flat child/attribute/operation/range arenas;
~~~~

### SRC-COMP-L1504-CDE23053695B

- Kind: `context`; source: `compiler-proposal.md:1504-1504`; target: `node:SCP2`; text SHA-256: `cde23053695baed63ce3cdab7d1ba4ba1085b16736b1cc9f8bd63d5fd1226fd6`.

~~~~markdown
- source fragments/anchors retained separately from target state;
~~~~

### SRC-COMP-L1505-BDE22D0848E6

- Kind: `context`; source: `compiler-proposal.md:1505-1505`; target: `node:SCP2`; text SHA-256: `bde22d0848e611a5dba5aa7fa6dc1286f3a19cc3a79e5846a93aed08ec800201`.

~~~~markdown
- client/server/style consume the same topology;
~~~~

### SRC-COMP-L1506-C1272562A40C

- Kind: `context`; source: `compiler-proposal.md:1506-1506`; target: `node:SCP2`; text SHA-256: `c1272562a40cb60d8e282fe678935eb2b49a5d5d71ceaf88d8df3cb326f00a5a`.

~~~~markdown
- no style semantics depend on runtime lowering retaining accidental geometry.
~~~~

### SRC-COMP-L1508-5D1CE4FA2351

- Kind: `context`; source: `compiler-proposal.md:1508-1508`; target: `node:SCP2`; text SHA-256: `5d1ce4fa23518e2a2e9f83c3fe4cc011976d9189340d27dedecf5b6e19b2722b`.

~~~~markdown
**Suggested predecessor:** `SCP1`.
~~~~

### SRC-COMP-L1510-BA4A0B975A3B

- Kind: `context`; source: `compiler-proposal.md:1510-1510`; target: `node:SCP2`; text SHA-256: `ba4a0b975a3b532f926057b24f8564535e12f1fbf74dc4f775cd66d850a8712e`.

~~~~markdown
**Suggested subblocks:** dense ID/data layout, region lowering, topology, dynamic feature facts, snippet edges, old runtime-IR consumer migration.
~~~~

### SRC-COMP-L1512-9E4F498B1EC3

- Kind: `acceptance`; source: `compiler-proposal.md:1512-1512`; target: `node:SCP2`; text SHA-256: `9e4f498b1ec3b7e58b5c5b15dba3a68ac97abf0920704c4bc9026ec0f27f4280`.

~~~~markdown
**Acceptance:** node access is O(1) by dense ID; style/client/server use one topology; source offset is not node identity; object-size/allocation budgets pass; no target helper/code layout lives in structure.
~~~~

### SRC-COMP-L1514-096831512D57

- Kind: `forbidden`; source: `compiler-proposal.md:1514-1514`; target: `node:SCP2`; text SHA-256: `096831512d575acbdd3af837c3b664b1d78a1ca0a24fb68c475a50a6632acf5c`.

~~~~markdown
**Forbidden:** source-offset IDs, duplicated client/server trees, compiler-local topology reconstruction, or Vue-shaped structural operations.
~~~~

### SRC-COMP-L1516-998863A07119

- Kind: `deletion`; source: `compiler-proposal.md:1516-1516`; target: `node:SCP2`; text SHA-256: `998863a0711932cef98be5e2b7e5d0c5d1230c74a8ba031dcb6c2ba41a2c7c1a`.

~~~~markdown
**Deletion/abort:** migrate consumers incrementally but keep one authority; abort shared mechanics that require framework semantic branches.
~~~~

### SRC-COMP-L1518-F52D711103D5

- Kind: `context`; source: `compiler-proposal.md:1518-1518`; target: `node:SCP2`; text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`.

~~~~markdown
---
~~~~
