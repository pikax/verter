# Exact operative source-clause attachment — SCP3

Schema: 1. Node: `SCP3`. Clause count: 17. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-COMP-L1624-3FABEB11D004

- Kind: `context`; source: `compiler-proposal.md:1624-1624`; target: `node:SCP3`; text SHA-256: `3fabeb11d004e5801fb32c15dd8d8ce1e24560eb7fec26a30682ca78739ea089`.

~~~~markdown
## `SCP3.md` — Svelte client Default compiler
~~~~

### SRC-COMP-L1626-93DA131910DB

- Kind: `context`; source: `compiler-proposal.md:1626-1626`; target: `node:SCP3`; text SHA-256: `93da131910db6571e9f0b969335b2ba1d5b7e83c2941b795b14147844f819c6b`.

~~~~markdown
**Intent:** implement client compilation from canonical semantics, topology, and style facts using demanded dependency/effect relations.
~~~~

### SRC-COMP-L1628-5DF21F6D9AFF

- Kind: `context`; source: `compiler-proposal.md:1628-1628`; target: `node:SCP3`; text SHA-256: `5df21f6d9aff1d4428f56789dc49334b57417ad7e3883f545ce6fa7478301637`.

~~~~markdown
**Problem:** transform/code generation can rediscover semantics, build multiple intermediate forms, and allocate broadly distributed object state.
~~~~

### SRC-COMP-L1630-56832D9ECFE1

- Kind: `context`; source: `compiler-proposal.md:1630-1630`; target: `node:SCP3`; text SHA-256: `56832d9ecfe1848d7a43f4e88a13f6f723f5dd01341c7a0613a3beb469414e2c`.

~~~~markdown
**Solution and architecture decisions:**
~~~~

### SRC-COMP-L1632-189F91E7AD4D

- Kind: `context`; source: `compiler-proposal.md:1632-1632`; target: `node:SCP3`; text SHA-256: `189f91e7ad4d8c6d41ec7c94334c38a9572e970e7b17c8d7eb28cc134a0c453c`.

~~~~markdown
- monomorphic Svelte+client executor;
~~~~

### SRC-COMP-L1633-3D9F68364AD3

- Kind: `requirement`; source: `compiler-proposal.md:1633-1633`; target: `node:SCP3`; text SHA-256: `3d9f68364ad337ce5d8ca5d69e9dcd3a2fe05b46b1e37d8b7868dc64e5031430`.

~~~~markdown
- demand-only dependency sets, effects, DOM operations, hydration, bindings, actions, transitions and animations;
~~~~

### SRC-COMP-L1634-3E155B8B7B57

- Kind: `context`; source: `compiler-proposal.md:1634-1634`; target: `node:SCP3`; text SHA-256: `3e155b8b7b57017896a14993a1924cd46f8b8cd70dc5a6e23cc95235372f44eb`.

~~~~markdown
- sparse/graph target state indexed by Svelte compiler identities;
~~~~

### SRC-COMP-L1635-2FBE8FF83A2A

- Kind: `context`; source: `compiler-proposal.md:1635-1635`; target: `node:SCP3`; text SHA-256: `2fbe8ff83a2a93a7e28e13589afb9a1f0504c2ab252fe395c85bf7d44ed3fbd3`.

~~~~markdown
- consume `SST2` match/scope facts once;
~~~~

### SRC-COMP-L1636-89AA798AB514

- Kind: `context`; source: `compiler-proposal.md:1636-1636`; target: `node:SCP3`; text SHA-256: `89aa798ab5143c3a563d153ca99beea2f0214435433f2d4be36b4f550810286c`.

~~~~markdown
- segmented emission and no-map specialization;
~~~~

### SRC-COMP-L1637-1CCD8BCE09E3

- Kind: `context`; source: `compiler-proposal.md:1637-1637`; target: `node:SCP3`; text SHA-256: `1ccd8bce09e3258b817d19fa6ccbb635d8a12a134823ccff84727dcbc98682bf`.

~~~~markdown
- no server plan or module compiler work.
~~~~

### SRC-COMP-L1639-E5ED001D2593

- Kind: `context`; source: `compiler-proposal.md:1639-1639`; target: `node:SCP3`; text SHA-256: `e5ed001d25934b7b6527d41b3a63d1dad6b3836f3f80f5fcb65503d87eca6f57`.

~~~~markdown
**Suggested predecessors:** `SCP2`, `SST2`.
~~~~

### SRC-COMP-L1641-146CE25935F6

- Kind: `context`; source: `compiler-proposal.md:1641-1641`; target: `node:SCP3`; text SHA-256: `146ce25935f6220f634079ab331a7bc0b6bb3904217f43e0b53515b8858e69e1`.

~~~~markdown
**Suggested subblocks:** static skeleton/DOM plan, reactive dependency/effects, blocks/snippets/components, directives/runtime operations, hydration, emission/maps/conformance.
~~~~

### SRC-COMP-L1643-F19916D3BDBD

- Kind: `acceptance`; source: `compiler-proposal.md:1643-1643`; target: `node:SCP3`; text SHA-256: `f19916d3bdbdfd0f02f5f8495dee14d92c199ddb56fc15932be5ec400c4e7359`.

~~~~markdown
**Acceptance:** locked client runtime/hydration/CSS/maps pass; no raw-source structural decisions; no duplicated style matching; target graph sizes and visits meet budgets.
~~~~

### SRC-COMP-L1645-33116DA99520

- Kind: `forbidden`; source: `compiler-proposal.md:1645-1645`; target: `node:SCP3`; text SHA-256: `33116da99520d1dd6b5aef1ef93b71f6c3917cda51b445347c8d6dcd1b3acd78`.

~~~~markdown
**Forbidden:** source-text transform heuristics, full reactive AST, server target state, or universal target operations.
~~~~

### SRC-COMP-L1647-FE5CE18F8C3D

- Kind: `deletion`; source: `compiler-proposal.md:1647-1647`; target: `node:SCP3`; text SHA-256: `fe5ce18f8c3d5fccfef49be5d7714938b908421d67f02f005266330af336e0f5`.

~~~~markdown
**Deletion/abort:** old client path deleted only at `SCP6` after parity.
~~~~

### SRC-COMP-L1649-F52D711103D5

- Kind: `context`; source: `compiler-proposal.md:1649-1649`; target: `node:SCP3`; text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`.

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
