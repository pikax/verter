# Exact operative source-clause attachment — CMP4

Schema: 1. Node: `CMP4`. Clause count: 20. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-COMP-L1049-D55B1BF6A927

- Kind: `context`; source: `compiler-proposal.md:1049-1049`; target: `node:CMP4`; text SHA-256: `d55b1bf6a92764cfda268eb6f1bcddf4d18f14f6f945e18deebb11bf64caf4d4`.

~~~~markdown
## `CMP4.md` — Segmented emission, qualified artifacts, assembly, and host integration
~~~~

### SRC-COMP-L1051-005A02561244

- Kind: `deletion`; source: `compiler-proposal.md:1051-1051`; target: `node:CMP4`; text SHA-256: `005a0256124478b2af9ad726e552603a15e41e6c78fbfa5b88f41461b59596bb`.

~~~~markdown
**Intent:** install the final shared compiler output path and remove framework topology from generic sessions.
~~~~

### SRC-COMP-L1053-C00D14648FC6

- Kind: `context`; source: `compiler-proposal.md:1053-1053`; target: `node:CMP4`; text SHA-256: `c00d14648fc60593ca63ce570eaed3ac145d529cc11499116f83440ae6f10216`.

~~~~markdown
**Problem:** ad hoc string generation, map work on no-map paths, fixed SFC output envelopes, and session-level framework assembly limit performance and extensibility.
~~~~

### SRC-COMP-L1055-56832D9ECFE1

- Kind: `context`; source: `compiler-proposal.md:1055-1055`; target: `node:CMP4`; text SHA-256: `56832d9ecfe1848d7a43f4e88a13f6f723f5dd01341c7a0613a3beb469414e2c`.

~~~~markdown
**Solution and architecture decisions:**
~~~~

### SRC-COMP-L1057-EFD460DEB644

- Kind: `context`; source: `compiler-proposal.md:1057-1057`; target: `node:CMP4`; text SHA-256: `efd460deb644824932964fe531963442a53d3de275d8db9bc4f7d8cb5530ea84`.

~~~~markdown
- define target-owned logical `EmitPlan` segments:
~~~~

### SRC-COMP-L1059-F026D8EF29C2

- Kind: `context`; source: `compiler-proposal.md:1059-1065`; target: `node:CMP4`; text SHA-256: `f026d8ef29c2eb3180f2358b49a28d4b6221a3a5b730046590013cb8a9b6dae6`.

~~~~markdown
```text
  SourceSlice
  GeneratedSlice + optional source anchor
  GeneratedUnmappedSlice
  StructuredInsertion
  ArtifactBoundary
  ```
~~~~

### SRC-COMP-L1067-0FB5C424256A

- Kind: `requirement`; source: `compiler-proposal.md:1067-1067`; target: `node:CMP4`; text SHA-256: `0fb5c424256ab5d3afa3ac4a17fd66a8137efda86dc78aa4fddef55a57c4dea3`.

~~~~markdown
- flatten once with exact or conservative sizing;
~~~~

### SRC-COMP-L1068-F10910E0EB0E

- Kind: `requirement`; source: `compiler-proposal.md:1068-1068`; target: `node:CMP4`; text SHA-256: `f10910e0eb0e1b36dbec7c99d13ccdaa569ab247dceabf3638691e3e39780d13`.

~~~~markdown
- generate runtime map segments during flattening only when requested;
~~~~

### SRC-COMP-L1069-E7F66BB30A2B

- Kind: `context`; source: `compiler-proposal.md:1069-1069`; target: `node:CMP4`; text SHA-256: `e7f66bb30a2b12b77f1cdf82395cbbc28a076dd304be519f3f7277721bf5d21a`.

~~~~markdown
- keep `NoMap` a physically specialized path with zero attributable map work;
~~~~

### SRC-COMP-L1070-3FE321AAC288

- Kind: `requirement`; source: `compiler-proposal.md:1070-1070`; target: `node:CMP4`; text SHA-256: `3fe321aac2881d21db9cdcb65999a1a7cd7615152006bac377c560910e03d158`.

~~~~markdown
- produce `CompileArtifactSet` with root, artifacts, relations, maps, diagnostics, provenance and exact basis;
~~~~

### SRC-COMP-L1071-43D57E73FDC3

- Kind: `context`; source: `compiler-proposal.md:1071-1071`; target: `node:CMP4`; text SHA-256: `43d57e73fdc3e1494a36d10425bdebdf470d68cb91321fc67d953a4aaddc9d64`.

~~~~markdown
- make the framework compiler own semantic module assembly;
~~~~

### SRC-COMP-L1072-7E55B5C9AD89

- Kind: `context`; source: `compiler-proposal.md:1072-1072`; target: `node:CMP4`; text SHA-256: `7e55b5c9ad8944f22921855980ef9a99829041825e2c68cdef0947abf3a94ac1`.

~~~~markdown
- make framework-host integration own Vite/Rollup/HMR/virtual IDs/manifests and external-style stages;
~~~~

### SRC-COMP-L1073-AFEDA8865EC7

- Kind: `context`; source: `compiler-proposal.md:1073-1073`; target: `node:CMP4`; text SHA-256: `afeda8865ec7eca426e18bf1d13bc27abc8b0e318415c32a3d01ddde323bfc9f`.

~~~~markdown
- keep OXC internal;
~~~~

### SRC-COMP-L1074-873051319E00

- Kind: `context`; source: `compiler-proposal.md:1074-1074`; target: `node:CMP4`; text SHA-256: `873051319e0046872282a8f4382d3ecafa912df3050c4c535cadc2fdb6c4067b`.

~~~~markdown
- keep custom blocks opaque unless an admitted future integration consumes them.
~~~~

### SRC-COMP-L1076-3E55132B5AF9

- Kind: `context`; source: `compiler-proposal.md:1076-1076`; target: `node:CMP4`; text SHA-256: `3e55132b5af94e37a68a8d80149a4268811dbce3befbbe6d78bc5d7dc82ec00c`.

~~~~markdown
**Suggested predecessor:** `CMP3`.
~~~~

### SRC-COMP-L1078-0DF4653947EA

- Kind: `deletion`; source: `compiler-proposal.md:1078-1078`; target: `node:CMP4`; text SHA-256: `0df4653947ea38dd596446cb0ce7f6c155bd5e098ba1b79b7096d9651c8c3bb6`.

~~~~markdown
**Suggested subblocks:** emit segment model, text flatten/map specialization, artifact graph, framework assembly adapter migration, host integration migration, old-output deletion ledger.
~~~~

### SRC-COMP-L1080-05BB2569AFAF

- Kind: `acceptance`; source: `compiler-proposal.md:1080-1080`; target: `node:CMP4`; text SHA-256: `05bb2569afaf86539f662615c40885941b4990f4862fc9038f3195d3a2d78e15`.

~~~~markdown
**Acceptance:** text-only/no-map requests do not build maps or native ASTs; framework modules are complete before the generic session receives them; host-specific decorations do not alter framework semantic decisions; artifact relations support client/server/CSS/metadata without schema changes; output copies/allocations meet locked budgets.
~~~~

### SRC-COMP-L1082-981CCE632EAD

- Kind: `forbidden`; source: `compiler-proposal.md:1082-1082`; target: `node:CMP4`; text SHA-256: `981cce632eadba96f80588bf837cef718e0ea0f83cb9d497f4e5bd6e1c2c1131`.

~~~~markdown
**Forbidden:** one generic SFC bundle, session knowledge of `_sfc_main` or framework wrappers, raw callback preprocessors, one universal map, or external AST ABI.
~~~~

### SRC-COMP-L1084-66013CDF6FEC

- Kind: `deletion`; source: `compiler-proposal.md:1084-1084`; target: `node:CMP4`; text SHA-256: `66013cdf6fec7d3f1f7dd3846457a6f4f137a601c2b1dc94f707fb9c91333267`.

~~~~markdown
**Deletion/abort:** adapters survive only with named VCP/SCP deletion owners; abort if artifact conversion loses map/provenance identity.
~~~~

### SRC-COMP-L1086-F52D711103D5

- Kind: `context`; source: `compiler-proposal.md:1086-1086`; target: `node:CMP4`; text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`.

~~~~markdown
---
~~~~
