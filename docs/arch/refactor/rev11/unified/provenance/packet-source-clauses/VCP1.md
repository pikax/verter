# Exact operative source-clause attachment — VCP1

Schema: 1. Node: `VCP1`. Clause count: 16. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-COMP-L1174-5D78E68CE415

- Kind: `context`; source: `compiler-proposal.md:1174-1174`; target: `node:VCP1`; text SHA-256: `5d78e68ce415afe44debf4901c4e38339c57fa34e2c612497bb4c6ad94d0594b`.

~~~~markdown
## `VCP1.md` — Canonical Vue semantic authority convergence
~~~~

### SRC-COMP-L1176-7E06E4C277C6

- Kind: `context`; source: `compiler-proposal.md:1176-1176`; target: `node:VCP1`; text SHA-256: `7e06e4c277c63af40bf62a897b1553181b4749d80974264f3b4a66441a086fb5`.

~~~~markdown
**Intent:** make one Vue semantic authority provide every framework fact used by compiler and tooling.
~~~~

### SRC-COMP-L1178-2D381772175F

- Kind: `requirement`; source: `compiler-proposal.md:1178-1178`; target: `node:VCP1`; text SHA-256: `2d381772175fc1f9664c08a5891badcdfd9512f740f8b18f5a150b8e5790b0ab`.

~~~~markdown
**Problem:** compiler-local import, binding, reactivity, directive, style, or dependency analysis can disagree with IDE/lint and duplicate expensive work.
~~~~

### SRC-COMP-L1180-56832D9ECFE1

- Kind: `context`; source: `compiler-proposal.md:1180-1180`; target: `node:VCP1`; text SHA-256: `56832d9ecfe1848d7a43f4e88a13f6f723f5dd01341c7a0613a3beb469414e2c`.

~~~~markdown
**Solution and architecture decisions:**
~~~~

### SRC-COMP-L1182-83B8ED4EBD11

- Kind: `context`; source: `compiler-proposal.md:1182-1182`; target: `node:VCP1`; text SHA-256: `83b8ed4ebd11d993d500d9d243e2dbe4f0981ea51b0a5c8963fd2e4e444849f9`.

~~~~markdown
- implement/extend Vue fact families inside the Vue semantic authority using shared `verter_analysis`/`type_info` machinery;
~~~~

### SRC-COMP-L1183-3E55B7577DA7

- Kind: `requirement`; source: `compiler-proposal.md:1183-1183`; target: `node:VCP1`; text SHA-256: `3e55b7577da712df897a5a2a02f971d99f4f59ca0718100f3510d8018e9a6415`.

~~~~markdown
- scopes, bindings, props/macros, component/element classification, directives, slots, reads/writes/dependencies, mutability, stability, purity and reactivity have one owner;
~~~~

### SRC-COMP-L1184-2DD3D92C39C4

- Kind: `context`; source: `compiler-proposal.md:1184-1184`; target: `node:VCP1`; text SHA-256: `2dd3d92c39c449090c9c0d8c6e615320b0218f12f365ec586fc66c6bef9590e7`.

~~~~markdown
- component-local framework-origin evidence supports contract-admitted literal framework imports, namespace/destructuring, immutable aliases and local alias chains visible in the SFC; it is distinct from resolved package provenance;
~~~~

### SRC-COMP-L1185-77B81A60F63C

- Kind: `context`; source: `compiler-proposal.md:1185-1185`; target: `node:VCP1`; text SHA-256: `77b81a60f63cd9b4f8894dbacd60a0edc626ebd7fda8c4c7c1c72772c3a15c20`.

~~~~markdown
- no node_modules/package/declaration/implementation loading under `Default`;
~~~~

### SRC-COMP-L1186-D3F761C9B08D

- Kind: `context`; source: `compiler-proposal.md:1186-1186`; target: `node:VCP1`; text SHA-256: `d3f761c9b08da85468787ce616e8e2cf6b501e848ca5ba8c4990e96573dac38a`.

~~~~markdown
- hot facts use compact dense summaries; provenance/explanations are sparse and demand-only;
~~~~

### SRC-COMP-L1187-3912F7BD1F9E

- Kind: `deletion`; source: `compiler-proposal.md:1187-1187`; target: `node:VCP1`; text SHA-256: `3912f7bd1f9ed7509af5e8f4e3c543fae6ae93a3b2a272321dbb50676b0c5553`.

~~~~markdown
- delete compiler-local reparse/scanner/analyzer paths as consumers migrate.
~~~~

### SRC-COMP-L1189-21D34E0AB4C5

- Kind: `context`; source: `compiler-proposal.md:1189-1189`; target: `node:VCP1`; text SHA-256: `21d34e0ab4c5ab89d22e6e9347c41e0de9e6bb4c8ab1c01a3f2fda0fab5a05e4`.

~~~~markdown
**Suggested predecessor:** `VCP0`.
~~~~

### SRC-COMP-L1191-DC7A1EFC4EEE

- Kind: `requirement`; source: `compiler-proposal.md:1191-1191`; target: `node:VCP1`; text SHA-256: `dc7a1efc4eeed357bede4d0a661a96c525b83473af8911c57d2e9095f6fbe1ce`.

~~~~markdown
**Suggested subblocks:** script/import facts, binding/scope facts, template/directive/slot facts, reactivity/dependency facts, compact storage/provenance, compiler-consumer cutover.
~~~~

### SRC-COMP-L1193-3A38E2F3FE97

- Kind: `acceptance`; source: `compiler-proposal.md:1193-1193`; target: `node:VCP1`; text SHA-256: `3a38e2f3fe97e3fe769f299c8df09bcec847795054120a00718714a8d7a1d24d`.

~~~~markdown
**Acceptance:** planted cheap alias cases produce the stronger correct fact in `Default`; same-spelled user functions and mutable aliases fail closed; compiler/IDE/lint observe one result; expression/import parse counts do not increase.
~~~~

### SRC-COMP-L1195-7AE90F803D9D

- Kind: `forbidden`; source: `compiler-proposal.md:1195-1195`; target: `node:VCP1`; text SHA-256: `7ae90f803d9d61f1b38898fd97febdc402bfb9e1a362cf39a4a694c6473b3318`.

~~~~markdown
**Forbidden:** a separate “fast compiler analyzer”, project traversal, tsgo, type-shape-only origin proof, or compiler-owned Vue semantics.
~~~~

### SRC-COMP-L1197-743C8303F57B

- Kind: `deletion`; source: `compiler-proposal.md:1197-1197`; target: `node:VCP1`; text SHA-256: `743c8303f57bd8effffb20259cec92cf76ce4b730e24b2308b86823e3281479d`.

~~~~markdown
**Deletion/abort:** delete duplicate analysis only after cross-consumer parity; return uncertain dynamic cases as `Unknown`.
~~~~

### SRC-COMP-L1199-F52D711103D5

- Kind: `context`; source: `compiler-proposal.md:1199-1199`; target: `node:VCP1`; text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`.

~~~~markdown
---
~~~~
