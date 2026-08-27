# Exact operative source-clause attachment — VCP6

Schema: 1. Node: `VCP6`. Clause count: 17. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-COMP-L1367-8B2CDC66F2A2

- Kind: `context`; source: `compiler-proposal.md:1367-1367`; target: `node:VCP6`; text SHA-256: `8b2cdc66f2a2d56514ebcd7a0c2021d068d96d6c8d8ed9cfc10adc7f5bdf1407`.

~~~~markdown
## `VCP6.md` — Vue module assembly, artifacts, host integration, and atomic cutover
~~~~

### SRC-COMP-L1369-1C06E1D68D26

- Kind: `deletion`; source: `compiler-proposal.md:1369-1369`; target: `node:VCP6`; text SHA-256: `1c06e1d68d263bf060e5fbe934005c02a0b105dcc7be50ae40fe52b4f8040713`.

~~~~markdown
**Intent:** make the Vue compiler produce complete framework artifacts and remove Vue semantics from generic session/host code.
~~~~

### SRC-COMP-L1371-FCDA4AE917A1

- Kind: `context`; source: `compiler-proposal.md:1371-1371`; target: `node:VCP6`; text SHA-256: `fcda4ae917a16d0ea6947038487a469dcb66015447c960d80156aba6cfffc4fb`.

~~~~markdown
**Problem:** target outputs can remain fragments requiring session-level assembly, style/custom-block handling can be ambiguous, and old/new target routes can coexist.
~~~~

### SRC-COMP-L1373-56832D9ECFE1

- Kind: `context`; source: `compiler-proposal.md:1373-1373`; target: `node:VCP6`; text SHA-256: `56832d9ecfe1848d7a43f4e88a13f6f723f5dd01341c7a0613a3beb469414e2c`.

~~~~markdown
**Solution and architecture decisions:**
~~~~

### SRC-COMP-L1375-CA09C8639A34

- Kind: `context`; source: `compiler-proposal.md:1375-1375`; target: `node:VCP6`; text SHA-256: `ca09c8639a342f0eb44e9d0bb62803f0f8d7f01183b73fd069685fc1ad847ffc`.

~~~~markdown
- assemble the complete Vue framework module inside the Vue runtime compiler;
~~~~

### SRC-COMP-L1376-C66D38BA22D2

- Kind: `context`; source: `compiler-proposal.md:1376-1376`; target: `node:VCP6`; text SHA-256: `c66d38ba22d219261fbf2ab6ccade6c14b8731a49dd89666dc27f8a8deb45c4c`.

~~~~markdown
- publish JS/CSS/maps/metadata/opaque custom-block attachments through `CompileArtifactSet`;
~~~~

### SRC-COMP-L1377-F378FA90EA59

- Kind: `requirement`; source: `compiler-proposal.md:1377-1377`; target: `node:VCP6`; text SHA-256: `f378fa90ea598a9664dd1134b5539d7b5f91b3d100cbfffdb17e1b5a1facb397`.

~~~~markdown
- route framework-host behavior through the exact `FrameworkHostIntegrationBackend`;
~~~~

### SRC-COMP-L1378-E835AD672882

- Kind: `context`; source: `compiler-proposal.md:1378-1378`; target: `node:VCP6`; text SHA-256: `e835ad672882b8a901550f749fc3d00ed29d17884f240be735eb6c20e50e3398`.

~~~~markdown
- compose VDOM/SSR/Vapor multi-target requests from shared prerequisites;
~~~~

### SRC-COMP-L1379-E383F36FF8E7

- Kind: `requirement`; source: `compiler-proposal.md:1379-1379`; target: `node:VCP6`; text SHA-256: `e383f36ff8e76c6b7e04d8c51aa18ce34ab8b250c5601d04c1b715bed9225c06`.

~~~~markdown
- preserve custom blocks as descriptors/attachments only;
~~~~

### SRC-COMP-L1380-B1B1900DD213

- Kind: `context`; source: `compiler-proposal.md:1380-1380`; target: `node:VCP6`; text SHA-256: `b1b1900dd213ef40d303c02287ab38cb6840d054ab2f4a7c28c0d957f3301bcd`.

~~~~markdown
- atomically route public/direct/prepared/managed compiler entry points to V2;
~~~~

### SRC-COMP-L1381-6FF766EEECA1

- Kind: `deletion`; source: `compiler-proposal.md:1381-1381`; target: `node:VCP6`; text SHA-256: `6ff766eeeca1aa945b61280783cea3809c410335cf7e95b99e4fbff30da1a1e0`.

~~~~markdown
- delete old Vue target walkers, session assembly, mixed outputs and temporary CCA adapters assigned to Vue.
~~~~

### SRC-COMP-L1383-1C4EFA89FD8E

- Kind: `context`; source: `compiler-proposal.md:1383-1383`; target: `node:VCP6`; text SHA-256: `1c4efa89fd8e1c01d08fbc2670460814368d375ad3fdf9221dde40c8981af934`.

~~~~markdown
**Suggested predecessors:** `VCP3`, `VCP4`, `VCP5`, `VST0`.
~~~~

### SRC-COMP-L1385-49F49B930AD5

- Kind: `deletion`; source: `compiler-proposal.md:1385-1385`; target: `node:VCP6`; text SHA-256: `49f49b930ad54828d13fc8def21ca8b432fe0826fc5502b00dbfddc2383840fa`.

~~~~markdown
**Suggested subblocks:** framework assembly, style/CSS artifacts, host adapters, custom-block opaque publication, route cutover, deletion and rollback.
~~~~

### SRC-COMP-L1387-DE2D082940FC

- Kind: `forbidden`; source: `compiler-proposal.md:1387-1387`; target: `node:VCP6`; text SHA-256: `de2d082940fc825d72f4e32158b1f7f43b30d757176f5f5b4035848feee06b54`.

~~~~markdown
**Acceptance:** generic session has no Vue module topology; all targets/maps/artifacts are complete; old and new paths never remain simultaneously authoritative; custom blocks are preserved without execution; host integrations cannot repair semantic output.
~~~~

### SRC-COMP-L1389-87D187266D7F

- Kind: `forbidden`; source: `compiler-proposal.md:1389-1389`; target: `node:VCP6`; text SHA-256: `87d187266d7fc2ef80263028ed4265e380f17c4c9912638c53c3cec564c91719`.

~~~~markdown
**Forbidden:** dynamic custom-block ABI, generic session assembly, hidden CSS pipeline, or per-host compiler semantics.
~~~~

### SRC-COMP-L1391-30F6A9EF1E00

- Kind: `deletion`; source: `compiler-proposal.md:1391-1391`; target: `node:VCP6`; text SHA-256: `30f6a9ef1e0035ff2ff9204839ce9a210be502b94afdc7a138b9f731d4786d92`.

~~~~markdown
**Deletion/abort:** this is the sole Vue cutover/deletion owner; abort on any unexplained target/artifact/map divergence.
~~~~

### SRC-COMP-L1393-F52D711103D5

- Kind: `context`; source: `compiler-proposal.md:1393-1393`; target: `node:VCP6`; text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`.

~~~~markdown
---
~~~~
