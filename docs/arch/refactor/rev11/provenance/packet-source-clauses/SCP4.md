# Exact operative source-clause attachment — SCP4

Schema: 1. Node: `SCP4`. Clause count: 16. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-COMP-L1651-11BAA9D3F797

- Kind: `context`; source: `compiler-proposal.md:1651-1651`; target: `node:SCP4`; text SHA-256: `11baa9d3f797d1eb215958a56f4e70942a5a55ee2323ee70fb51d98f397f0bd7`.

~~~~markdown
## `SCP4.md` — Svelte server Default compiler
~~~~

### SRC-COMP-L1653-150E9718456F

- Kind: `context`; source: `compiler-proposal.md:1653-1653`; target: `node:SCP4`; text SHA-256: `150e9718456f5068c717d10de9366a779f0279ac95965d509246070ef0cff14f`.

~~~~markdown
**Intent:** implement server compilation with shared semantics/structure/style and zero client-effect work.
~~~~

### SRC-COMP-L1655-7F80585FF698

- Kind: `context`; source: `compiler-proposal.md:1655-1655`; target: `node:SCP4`; text SHA-256: `7f80585ff698cd7cddaa4fd2396cf7584a40c2c3118e8c6401283fdcd1b97486`.

~~~~markdown
**Problem:** server compilation can inherit client data structures or repeat shared analysis.
~~~~

### SRC-COMP-L1657-56832D9ECFE1

- Kind: `context`; source: `compiler-proposal.md:1657-1657`; target: `node:SCP4`; text SHA-256: `56832d9ecfe1848d7a43f4e88a13f6f723f5dd01341c7a0613a3beb469414e2c`.

~~~~markdown
**Solution and architecture decisions:**
~~~~

### SRC-COMP-L1659-AE18A21CDB86

- Kind: `context`; source: `compiler-proposal.md:1659-1659`; target: `node:SCP4`; text SHA-256: `ae18a21cdb861957a86e64a6f837604ac32af501173955652e5060903b10c347`.

~~~~markdown
- monomorphic Svelte+server executor;
~~~~

### SRC-COMP-L1660-F90898FFD4D7

- Kind: `context`; source: `compiler-proposal.md:1660-1660`; target: `node:SCP4`; text SHA-256: `f90898ffd4d716f59555dce17944a5f2874ca8d1db5249e79a78bbab617085d7`.

~~~~markdown
- consume shared structure and style facts;
~~~~

### SRC-COMP-L1661-AFC4C6FAA382

- Kind: `context`; source: `compiler-proposal.md:1661-1661`; target: `node:SCP4`; text SHA-256: `afc4c6faa382625e93c87dcadf34ae271994d17d3bad20a1d4058bfe6704a597`.

~~~~markdown
- segment-oriented server emission and minimal server plan;
~~~~

### SRC-COMP-L1662-2A68551ED4AB

- Kind: `context`; source: `compiler-proposal.md:1662-1662`; target: `node:SCP4`; text SHA-256: `2a68551ed4ab58d8b0ec3a8768ad914a9ffad993cfc8f9989efc3b11e836a9d3`.

~~~~markdown
- zero client effects, DOM plan, transitions/actions/hydration work;
~~~~

### SRC-COMP-L1663-5EBB3B0FECA4

- Kind: `context`; source: `compiler-proposal.md:1663-1663`; target: `node:SCP4`; text SHA-256: `5ebb3b0feca4e17f9be52b376ccef5cd014ba7f3b60737d87c36b332fe95c15d`.

~~~~markdown
- share prerequisites with client when both requested.
~~~~

### SRC-COMP-L1665-E5ED001D2593

- Kind: `context`; source: `compiler-proposal.md:1665-1665`; target: `node:SCP4`; text SHA-256: `e5ed001d25934b7b6527d41b3a63d1dad6b3836f3f80f5fcb65503d87eca6f57`.

~~~~markdown
**Suggested predecessors:** `SCP2`, `SST2`.
~~~~

### SRC-COMP-L1667-A243F43CC49E

- Kind: `context`; source: `compiler-proposal.md:1667-1667`; target: `node:SCP4`; text SHA-256: `a243f43cc49ea85182abe660205ac6c85e98ef7d3f549a350393378d323f885d`.

~~~~markdown
**Suggested subblocks:** server text/escaping, elements/components/slots/blocks, style/head/module relations, maps, client+server sharing, performance proof.
~~~~

### SRC-COMP-L1669-1FBB5C5117ED

- Kind: `acceptance`; source: `compiler-proposal.md:1669-1669`; target: `node:SCP4`; text SHA-256: `1fbb5c5117ed4738b15fb874566f832d019ca2b1013bc1a21773ad16def8e120`.

~~~~markdown
**Acceptance:** server behavior/maps/CSS pass; client target counters are zero; combined client/server requests do not repeat parse/semantic/style/topology work.
~~~~

### SRC-COMP-L1671-3CFA79B2A96A

- Kind: `forbidden`; source: `compiler-proposal.md:1671-1671`; target: `node:SCP4`; text SHA-256: `3cfa79b2a96a25b702d0696a1fdc9afe877cd519737e05019931d86df154050c`.

~~~~markdown
**Forbidden:** client graph reuse by convenience, duplicate style matching, or full server target tree without evidence.
~~~~

### SRC-COMP-L1673-A9A147E78EEB

- Kind: `deletion`; source: `compiler-proposal.md:1673-1673`; target: `node:SCP4`; text SHA-256: `a9a147e78eeb3e0d3cbd4c207dc628bbeb101f48be32592d98cad73d9aa4a904`.

~~~~markdown
**Deletion/abort:** old server path deleted at `SCP6` after parity.
~~~~

### SRC-COMP-L1675-F52D711103D5

- Kind: `context`; source: `compiler-proposal.md:1675-1675`; target: `node:SCP4`; text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`.

~~~~markdown
---
~~~~

### SRC-LEGACY-TRANSFER-1B11EAEACCFE

- Kind: `requirement`; source: `legacy-architecture-transfers.md:558-563`; target: `node:SCP0`; text SHA-256: `196151f1cc3d11ad89808f4d1c20e806d32733cbfefc5ae47875e2d2aded0d98`.

~~~~markdown
### LEGACY-TRANSFER-1B11EAEACCFE

- Original path: `docs/arch/svelte-native-compiler-plan.md`; Git blob: `1b11eaeaccfea6baaad3684710026923b734bb88`; exact source SHA-256: `e96ca99c36787fbb0d9d29300601c3a58d653a0fb57f89a560a24080662dd7ad`.
- Exact retained source: `sources/legacy-architecture-transfers/svelte-native-compiler-plan.md`.
- Applicable authority: `SCP0`, `SCP1`, `SCP2`, `SCP3`, `SCP4`, `SCP5`, `SCP6`, `SCP7`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~
