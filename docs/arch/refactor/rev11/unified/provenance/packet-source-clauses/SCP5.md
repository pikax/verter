# Exact operative source-clause attachment — SCP5

Schema: 1. Node: `SCP5`. Clause count: 12. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-COMP-L1677-7009EA5C83DF

- Kind: `context`; source: `compiler-proposal.md:1677-1677`; target: `node:SCP5`; text SHA-256: `7009ea5c83dfd1c5e9ebf5fc66fe2290e47c524da0f59db722ff459cb4cbd5ad`.

~~~~markdown
## `SCP5.md` — Svelte module compiler for `.svelte.js` and `.svelte.ts`
~~~~

### SRC-COMP-L1679-E6C8ACB752DA

- Kind: `context`; source: `compiler-proposal.md:1679-1679`; target: `node:SCP5`; text SHA-256: `e6c8acb752da13a7605db99568026eea51f8876b17c8d5aa81dc57127a25380a`.

~~~~markdown
**Intent:** compile module-rune semantics through the JS/TS frontend without forcing module files through the component carrier.
~~~~

### SRC-COMP-L1681-4EE977BCFA1A

- Kind: `context`; source: `compiler-proposal.md:1681-1681`; target: `node:SCP5`; text SHA-256: `4ee977bcfa1a3378fecc0f03d4b08798b4c7c528d81479547a004c78c7ccb583`.

~~~~markdown
**Problem:** module compilation is easy to omit or implement with raw-text scanning and does not naturally belong to an SFC frontend.
~~~~

### SRC-COMP-L1683-56832D9ECFE1

- Kind: `context`; source: `compiler-proposal.md:1683-1683`; target: `node:SCP5`; text SHA-256: `56832d9ecfe1848d7a43f4e88a13f6f723f5dd01341c7a0613a3beb469414e2c`.

~~~~markdown
**Solution and architecture decisions:**
~~~~

### SRC-COMP-L1685-795379E04995

- Kind: `context`; source: `compiler-proposal.md:1685-1693`; target: `node:SCP5`; text SHA-256: `795379e04995922912c6aa4e06a0f1758746c6a702b8638c36ec7d457e82eb60`.

~~~~markdown
```text
OXC JS/TS frontend
    +
Svelte semantic profile/authority
    ↓
Svelte module semantic facts
    ↓
Svelte module target planning/emission
```
~~~~

### SRC-COMP-L1695-12CE1DB415BC

- Kind: `requirement`; source: `compiler-proposal.md:1695-1695`; target: `node:SCP5`; text SHA-256: `12ce1db415bc84a4de150306b83e09f43267c1702e316d8a672b24a09368378c`.

~~~~markdown
OXC remains internal. Module semantics reuse canonical runes/bindings/dependencies but own their target-specific rewriting and artifacts.
~~~~

### SRC-COMP-L1697-5D1CE4FA2351

- Kind: `context`; source: `compiler-proposal.md:1697-1697`; target: `node:SCP5`; text SHA-256: `5d1ce4fa23518e2a2e9f83c3fe4cc011976d9189340d27dedecf5b6e19b2722b`.

~~~~markdown
**Suggested predecessor:** `SCP1`.
~~~~

### SRC-COMP-L1699-04FD191B0F63

- Kind: `context`; source: `compiler-proposal.md:1699-1699`; target: `node:SCP5`; text SHA-256: `04fd191b0f63bfa2d36bcaaec9655376f2447fb4af12c6140ec389e8f3cd9a77`.

~~~~markdown
**Suggested subblocks:** module activation/options, rune/module facts, target plan, emission/maps, diagnostics, differential/performance tests.
~~~~

### SRC-COMP-L1701-60CA5CF2C141

- Kind: `acceptance`; source: `compiler-proposal.md:1701-1701`; target: `node:SCP5`; text SHA-256: `60ca5cf2c1412855c5a9e342d162e3b6f279b000afc0f46c86e503e91a1d7e04`.

~~~~markdown
**Acceptance:** no component frontend or source-string scanner is used; locked module behavior/maps pass; ordinary JS/TS remains unaffected when the Svelte module profile is inactive.
~~~~

### SRC-COMP-L1703-5182E671EE15

- Kind: `forbidden`; source: `compiler-proposal.md:1703-1703`; target: `node:SCP5`; text SHA-256: `5182e671ee15e4e96eacfa5861daf63dccdaf862a0b2e451bce23bc76c025367`.

~~~~markdown
**Forbidden:** SFC wrappers, filename-only semantic activation without the locked contract, external AST output, or duplicated rune analysis.
~~~~

### SRC-COMP-L1705-5ECE75334D75

- Kind: `deletion`; source: `compiler-proposal.md:1705-1705`; target: `node:SCP5`; text SHA-256: `5ece75334d7524a997173b30cacff75f3da8bc86ef0ea26ef21060c86ee9887a`.

~~~~markdown
**Deletion/abort:** delete old module transform paths after parity; keep unsupported cells explicit.
~~~~

### SRC-COMP-L1707-F52D711103D5

- Kind: `context`; source: `compiler-proposal.md:1707-1707`; target: `node:SCP5`; text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`.

~~~~markdown
---
~~~~
