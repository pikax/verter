# Exact operative source-clause attachment — SCP6

Schema: 1. Node: `SCP6`. Clause count: 17. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-COMP-L1709-E6AD95E47C48

- Kind: `context`; source: `compiler-proposal.md:1709-1709`; target: `node:SCP6`; text SHA-256: `e6ad95e47c48a13b435f0047bf81a5dd36ae451bbe715b40e3156df50c242ad0`.

~~~~markdown
## `SCP6.md` — Svelte assembly, artifacts, host integration, and atomic cutover
~~~~

### SRC-COMP-L1711-8962DD639DEA

- Kind: `deletion`; source: `compiler-proposal.md:1711-1711`; target: `node:SCP6`; text SHA-256: `8962dd639deae9ddde8f168c984e24be4d749db729aa94ad9a2d90765b4602cf`.

~~~~markdown
**Intent:** publish complete Svelte artifacts and remove framework semantics from generic session/host code.
~~~~

### SRC-COMP-L1713-36EC84A71466

- Kind: `context`; source: `compiler-proposal.md:1713-1713`; target: `node:SCP6`; text SHA-256: `36ec84a714669a150ffa91350e0d6c4f4ed00fa2c92b7f6c44017b0ad5a7f947`.

~~~~markdown
**Problem:** client/server/module/style outputs can remain separately assembled, and experimental/old paths may coexist.
~~~~

### SRC-COMP-L1715-56832D9ECFE1

- Kind: `context`; source: `compiler-proposal.md:1715-1715`; target: `node:SCP6`; text SHA-256: `56832d9ecfe1848d7a43f4e88a13f6f723f5dd01341c7a0613a3beb469414e2c`.

~~~~markdown
**Solution and architecture decisions:**
~~~~

### SRC-COMP-L1717-38846151AF93

- Kind: `context`; source: `compiler-proposal.md:1717-1717`; target: `node:SCP6`; text SHA-256: `38846151af93a9029abd2c353d307871daa796312eb1f2caa7a30ae09e9b4f2c`.

~~~~markdown
- assemble complete client/server/module artifacts inside the Svelte compiler;
~~~~

### SRC-COMP-L1718-05497E18BDB8

- Kind: `context`; source: `compiler-proposal.md:1718-1718`; target: `node:SCP6`; text SHA-256: `05497e18bdb882fcda836aa3b0078af7067b754c2167972d093408a4ae0eab0d`.

~~~~markdown
- publish JS/CSS/maps/metadata through `CompileArtifactSet`;
~~~~

### SRC-COMP-L1719-4EB36E25A6A7

- Kind: `context`; source: `compiler-proposal.md:1719-1719`; target: `node:SCP6`; text SHA-256: `4eb36e25a6a7e339d0db6eb8da20eca38a1b7898508f70f8b5492dc1f6cd214d`.

~~~~markdown
- route CSS injection/extraction, HMR and virtual-module policy through framework-host integration;
~~~~

### SRC-COMP-L1720-EE68DD63B100

- Kind: `context`; source: `compiler-proposal.md:1720-1720`; target: `node:SCP6`; text SHA-256: `ee68dd63b1009d5cb3aa39c13988f681552a1a64335329590ba923d303af6121`.

~~~~markdown
- share client/server prerequisites and style facts;
~~~~

### SRC-COMP-L1721-07B322271776

- Kind: `context`; source: `compiler-proposal.md:1721-1721`; target: `node:SCP6`; text SHA-256: `07b32227177618b9f8299d49e258c4c4868db25990539d4dfcf7511fabae933e`.

~~~~markdown
- atomically cut direct/prepared/managed/public routes to V2;
~~~~

### SRC-COMP-L1722-9FB4B9AF4D59

- Kind: `deletion`; source: `compiler-proposal.md:1722-1722`; target: `node:SCP6`; text SHA-256: `9fb4b9af4d5959fdd17396dc42dcfb2c9320f5511e10928fc64103da0efd0584`.

~~~~markdown
- delete experimental compiler representations, style matcher routes, session assembly and temporary CCA adapters assigned to Svelte.
~~~~

### SRC-COMP-L1724-DC694BD37802

- Kind: `context`; source: `compiler-proposal.md:1724-1724`; target: `node:SCP6`; text SHA-256: `dc694bd37802ce9a7635b1ef92674522373fa6f5be398f934f881e224c4d6fe5`.

~~~~markdown
**Suggested predecessors:** `SCP3`, `SCP4`, `SCP5`, `SST2`.
~~~~

### SRC-COMP-L1726-782D53BC1528

- Kind: `deletion`; source: `compiler-proposal.md:1726-1726`; target: `node:SCP6`; text SHA-256: `782d53bc152819142739bd6b60264999ff0bc104d07abd129daea175b7d269f7`.

~~~~markdown
**Suggested subblocks:** artifact assembly, style publication, host integration, multi-target orchestration, route cutover, deletion/rollback.
~~~~

### SRC-COMP-L1728-6FB773C9229B

- Kind: `acceptance`; source: `compiler-proposal.md:1728-1728`; target: `node:SCP6`; text SHA-256: `6fb773c9229bc08bce7b1ba95d163c19a9626d5a5139a45b27d810bef7cf2792`.

~~~~markdown
**Acceptance:** generic session contains no Svelte module topology; all compiler products are complete and map-qualified; one style-match fact product serves all targets; no old compiler authority remains reachable.
~~~~

### SRC-COMP-L1730-3613C6105AB8

- Kind: `forbidden`; source: `compiler-proposal.md:1730-1730`; target: `node:SCP6`; text SHA-256: `3613c6105ab88dc70c05d65515bca5d14b8e0b234f81ecb15cbdac132a65159e`.

~~~~markdown
**Forbidden:** compatibility dual-running, host repair of incomplete semantics, native preprocessor, or fixed SFC artifact schema.
~~~~

### SRC-COMP-L1732-D014EF862CCC

- Kind: `deletion`; source: `compiler-proposal.md:1732-1732`; target: `node:SCP6`; text SHA-256: `d014ef862ccc264f28441d308efa324c023160c4ddea280da8978ea7c26ac1d0`.

~~~~markdown
**Deletion/abort:** sole Svelte compiler cutover/deletion owner; abort on unexplained target/artifact/map divergence.
~~~~

### SRC-COMP-L1734-F52D711103D5

- Kind: `context`; source: `compiler-proposal.md:1734-1734`; target: `node:SCP6`; text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`.

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
