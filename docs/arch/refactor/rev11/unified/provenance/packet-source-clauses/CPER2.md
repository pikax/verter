# Exact operative source-clause attachment — CPER2

Schema: 1. Node: `CPER2`. Clause count: 22. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-COMP-L1088-07CC4EC112C8

- Kind: `context`; source: `compiler-proposal.md:1088-1088`; target: `node:CPER2`; text SHA-256: `07cc4ec112c8fd08de92ba875894d5406f2442909db46b99c0cd7825b19791f7`.

~~~~markdown
## `CPER2.md` — Shared compiler physical-execution and zero-work terminal
~~~~

### SRC-COMP-L1090-33D9290AB28C

- Kind: `context`; source: `compiler-proposal.md:1090-1090`; target: `node:CPER2`; text SHA-256: `33d9290ab28ca9977113cf45dc2d225e951b61e7a152ca10d56a4ccc1759302f`.

~~~~markdown
**Intent:** verify the common compiler substrate before framework V2 trains depend on it.
~~~~

### SRC-COMP-L1092-FF883E0BE0E7

- Kind: `context`; source: `compiler-proposal.md:1092-1092`; target: `node:CPER2`; text SHA-256: `ff883e0be0e7ee87237871f7e97c6b67a0333d5e18c8144ec0f3f8be7f347114`.

~~~~markdown
**Problem:** clean contracts can still produce physically materialized multi-pass implementations, hidden map work, or memory regressions.
~~~~

### SRC-COMP-L1094-FDB918AF6ED7

- Kind: `context`; source: `compiler-proposal.md:1094-1094`; target: `node:CPER2`; text SHA-256: `fdb918af6ed73ef8e76ef9e62d822e2c2253ab495d4f9cac35173a11fe32433e`.

~~~~markdown
**Solution and architecture decisions:** run architecture-budget tests and canaries over the accepted common engine.
~~~~

### SRC-COMP-L1096-C431D79C12CA

- Kind: `requirement`; source: `compiler-proposal.md:1096-1096`; target: `node:CPER2`; text SHA-256: `c431d79c12cacfd2aab7cfa590a62c039c8c5d73339482f2c930dd0c84ee9f75`.

~~~~markdown
**Required laws:**
~~~~

### SRC-COMP-L1098-C2215B4978ED

- Kind: `requirement`; source: `compiler-proposal.md:1098-1098`; target: `node:CPER2`; text SHA-256: `c2215b4978ed7fdb0bb61707e065578e1ea540f0eabe64904f4cadb0d1b3bb5f`.

~~~~markdown
- no redundant authoritative parse of the same exact region/grammar product;
~~~~

### SRC-COMP-L1099-C49170602AE1

- Kind: `context`; source: `compiler-proposal.md:1099-1099`; target: `node:CPER2`; text SHA-256: `c49170602ae123bb0519157fdaa73648d3ebad220a116b788dd82d75c1152eda`.

~~~~markdown
- no semantic raw-source searching after parse;
~~~~

### SRC-COMP-L1100-787D7D1F13EF

- Kind: `context`; source: `compiler-proposal.md:1100-1100`; target: `node:CPER2`; text SHA-256: `787d7d1f13efe706ad0cd0d6b98a46ca87a000d07586d4d22edd0810a3e02fa5`.

~~~~markdown
- no compiler-local duplicate framework analysis;
~~~~

### SRC-COMP-L1101-0620B5C8A49E

- Kind: `context`; source: `compiler-proposal.md:1101-1101`; target: `node:CPER2`; text SHA-256: `0620b5c8a49ef354346456b696325d0e426058f0e6a1911fa99a057095f9f3d8`.

~~~~markdown
- no lossless/recovery allocation in valid strict compilation;
~~~~

### SRC-COMP-L1102-6AAD62A8AE1A

- Kind: `context`; source: `compiler-proposal.md:1102-1102`; target: `node:CPER2`; text SHA-256: `6aad62a8ae1ab60bcac2da8a6c1e44f508deab221d2df95632ef2fdab41af41c`.

~~~~markdown
- no per-node dynamic target dispatch;
~~~~

### SRC-COMP-L1103-ADA7285CE3CB

- Kind: `context`; source: `compiler-proposal.md:1103-1103`; target: `node:CPER2`; text SHA-256: `ada7285ce3cb4ad35ab6e64272296dea1cf23a1791a715257193ca76dfe9ffbd`.

~~~~markdown
- no map work when maps are disabled;
~~~~

### SRC-COMP-L1104-05577BF9C1E7

- Kind: `requirement`; source: `compiler-proposal.md:1104-1104`; target: `node:CPER2`; text SHA-256: `05577bf9c1e76c6dbf7cdb74909a6ed94b45744070fa4135ad35c7b1a6f523d9`.

~~~~markdown
- no client effect planning for server-only targets;
~~~~

### SRC-COMP-L1105-C3225DE17E1A

- Kind: `context`; source: `compiler-proposal.md:1105-1105`; target: `node:CPER2`; text SHA-256: `c3225de17e1aca5876a18958fe87d22cf3cdde29d4212fcab22f6c45cc3884bd`.

~~~~markdown
- unknown facts cannot enable optimization;
~~~~

### SRC-COMP-L1106-A0C37630581B

- Kind: `context`; source: `compiler-proposal.md:1106-1106`; target: `node:CPER2`; text SHA-256: `a0c37630581b036a9f911e9121a8ff79bb025aa8a60aecf3323b716be065afe5`.

~~~~markdown
- raw source copy bytes are zero for representation ownership;
~~~~

### SRC-COMP-L1107-553EAEB4A1E2

- Kind: `requirement`; source: `compiler-proposal.md:1107-1107`; target: `node:CPER2`; text SHA-256: `553eaeb4a1e21696e7000010f195342f339301e35160d5a4dcd657bfd4feded0`.

~~~~markdown
- incremental/prepared reuse validates exact basis.
~~~~

### SRC-COMP-L1109-37C00B96FE9F

- Kind: `context`; source: `compiler-proposal.md:1109-1109`; target: `node:CPER2`; text SHA-256: `37c00b96fe9f6b76122af3b7cf5c12e8eb61de585c2ef2ac0a04a820086a84c7`.

~~~~markdown
**Budgets:** node sizes, source-sized visits, region/graph visits, allocations, bytes/lifetime, emission copies, map segments, cancellation waste, and disabled instrumentation overhead.
~~~~

### SRC-COMP-L1111-900049A6469D

- Kind: `context`; source: `compiler-proposal.md:1111-1111`; target: `node:CPER2`; text SHA-256: `900049a6469d27cf62ce6cc6f262f40abda6347d8bdcd35633facda73caaf971`.

~~~~markdown
**Suggested predecessors:** `CMP4`, `CPER1`.
~~~~

### SRC-COMP-L1113-24EBCDF1E578

- Kind: `context`; source: `compiler-proposal.md:1113-1113`; target: `node:CPER2`; text SHA-256: `24ebcdf1e57895b38d3b9b8dbeeb85368b423087734942505bcb6f7f8e0d0f7e`.

~~~~markdown
**Suggested subblocks:** strict-path canary, maps/no-maps canary, server/client demand canary, multi-target sharing canary, memory/RSS soak, exact-candidate architecture review.
~~~~

### SRC-COMP-L1115-506B0A604FD0

- Kind: `acceptance`; source: `compiler-proposal.md:1115-1115`; target: `node:CPER2`; text SHA-256: `506b0a604fd09eab1eead772eb1446e40bf941f28868e548483572ef361b4535`.

~~~~markdown
**Acceptance:** all laws pass mechanically; every budget has a pinned value and equivalent-work basis; no implementation fix is made inside the terminal candidate.
~~~~

### SRC-COMP-L1117-12BA562A1F66

- Kind: `forbidden`; source: `compiler-proposal.md:1117-1117`; target: `node:CPER2`; text SHA-256: `12ba562a1f66218181e5d80ea62526e9a725d0f52a839e19e44980541235220e`.

~~~~markdown
**Forbidden:** changing gates after measurement, treating “one pass” as a universal law, or accepting unexplained extra work because wall time remains noisy.
~~~~

### SRC-COMP-L1119-46822256486E

- Kind: `deletion`; source: `compiler-proposal.md:1119-1119`; target: `node:CPER2`; text SHA-256: `46822256486e853673e534082bcbe88d2f60cb2439c7eca31dc02b8234843b4b`.

~~~~markdown
**Deletion/abort:** findings return to `CMP0`–`CMP4` or `CPER1`; this terminal deletes nothing.
~~~~

### SRC-COMP-L1121-F52D711103D5

- Kind: `context`; source: `compiler-proposal.md:1121-1121`; target: `node:CPER2`; text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`.

~~~~markdown
---
~~~~
