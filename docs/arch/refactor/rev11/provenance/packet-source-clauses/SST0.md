# Exact operative source-clause attachment — SST0

Schema: 1. Node: `SST0`. Clause count: 16. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-COMP-L1520-9B7600238C92

- Kind: `context`; source: `compiler-proposal.md:1520-1520`; target: `node:SST0`; text SHA-256: `9b7600238c92243746170bfbfe7d912983350e0a2f43ca646cafe76eb037f12d`.

~~~~markdown
## `SST0.md` — Svelte framework style semantics and source-stage integration
~~~~

### SRC-COMP-L1522-11C374F04BBB

- Kind: `context`; source: `compiler-proposal.md:1522-1522`; target: `node:SST0`; text SHA-256: `11c374f04bbb3bc98da5b65830516a94d85b5309db5de351cfb0d492d2a89e2a`.

~~~~markdown
**Intent:** consume J-owned CSS products and establish one Svelte style-semantic authority before matching/planning.
~~~~

### SRC-COMP-L1524-EEBDFAE6E625

- Kind: `context`; source: `compiler-proposal.md:1524-1524`; target: `node:SST0`; text SHA-256: `eebdfae6e625024e865f896f49c48188e04aaf2480dd0803923ad2b8c35852b9`.

~~~~markdown
**Problem:** a compiler-local CSS grammar/matcher or ambiguous preprocessing stage can create duplicate syntax and incorrect map/scoping behavior.
~~~~

### SRC-COMP-L1526-56832D9ECFE1

- Kind: `context`; source: `compiler-proposal.md:1526-1526`; target: `node:SST0`; text SHA-256: `56832d9ecfe1848d7a43f4e88a13f6f723f5dd01341c7a0613a3beb469414e2c`.

~~~~markdown
**Solution and architecture decisions:**
~~~~

### SRC-COMP-L1528-62E4E219E3FE

- Kind: `context`; source: `compiler-proposal.md:1528-1528`; target: `node:SST0`; text SHA-256: `62e4e219e3fe80dfd19c486b24aaaa784b352604a7ae153d3b191e9a248bb88c`.

~~~~markdown
- consume J `StyleSyntaxIr` and neutral facts;
~~~~

### SRC-COMP-L1529-87B3117B65F6

- Kind: `context`; source: `compiler-proposal.md:1529-1529`; target: `node:SST0`; text SHA-256: `87b3117b65f6a7088ee5a1dd8920ec7f056c9ad2c26104d6042a833885992f47`.

~~~~markdown
- own Svelte-specific global/local semantics, keyframe meaning, scope-hash inputs, style injection/extraction facts and diagnostics;
~~~~

### SRC-COMP-L1530-BEC067C81C99

- Kind: `requirement`; source: `compiler-proposal.md:1530-1530`; target: `node:SST0`; text SHA-256: `bec067c81c99d6a530acc851f3f82c1067e2e2d69fadbe2197176ca196717d99`.

~~~~markdown
- connect processed CSS to authored dialect through exact external-stage maps/read sets;
~~~~

### SRC-COMP-L1531-9E29FC0EE4BD

- Kind: `context`; source: `compiler-proposal.md:1531-1531`; target: `node:SST0`; text SHA-256: `9e29fc0ee4bdedaf8ed1baf14a3354a50bbc235bc375ba2452a5f00a3ea6ed63`.

~~~~markdown
- no native preprocessors;
~~~~

### SRC-COMP-L1532-33C536690F2B

- Kind: `context`; source: `compiler-proposal.md:1532-1532`; target: `node:SST0`; text SHA-256: `33c536690f2b23079ca9c7e409c0901529f9268346fc9f34eba524cd9df75c6c`.

~~~~markdown
- create one style identity and scope basis shared by client/server/CSS emission;
~~~~

### SRC-COMP-L1533-081F19C27DB1

- Kind: `requirement`; source: `compiler-proposal.md:1533-1533`; target: `node:SST0`; text SHA-256: `081f19c27db121c4b986591c2175fd7d388c8beb2ece2bf826721058af4706bf`.

~~~~markdown
- expose the exact inputs required by selector matching without performing it here.
~~~~

### SRC-COMP-L1535-5D1CE4FA2351

- Kind: `context`; source: `compiler-proposal.md:1535-1535`; target: `node:SST0`; text SHA-256: `5d1ce4fa23518e2a2e9f83c3fe4cc011976d9189340d27dedecf5b6e19b2722b`.

~~~~markdown
**Suggested predecessor:** `SCP1`.
~~~~

### SRC-COMP-L1537-93540A246EB5

- Kind: `deletion`; source: `compiler-proposal.md:1537-1537`; target: `node:SST0`; text SHA-256: `93540a246eb5542a329d97e44ac2ab46f4f74b25c224027d9ead0dea9dca0b0e`.

~~~~markdown
**Suggested subblocks:** J integration, framework style facts, scope/hash identity, external-stage/maps, client/server style-demand contract, legacy parser/scanner deletion.
~~~~

### SRC-COMP-L1539-C8330BC94306

- Kind: `acceptance`; source: `compiler-proposal.md:1539-1539`; target: `node:SST0`; text SHA-256: `c8330bc943063f65f3422a1e1cbbfcbc32b05e5e8d0e03274071abb78a62f0d4`.

~~~~markdown
**Acceptance:** one CSS parse per exact style block/grammar product; no compiler-local grammar/scanner; client/server share style identity; preprocessing ambiguity returns `NeedInputs`.
~~~~

### SRC-COMP-L1541-71DE4812C345

- Kind: `forbidden`; source: `compiler-proposal.md:1541-1541`; target: `node:SST0`; text SHA-256: `71de4812c345d0454398834cfc67d8d06e82db93618d672dc85580916c44e467`.

~~~~markdown
**Forbidden:** raw CSS rescans, runtime-IR-owned style semantics, native preprocessors, or selector pruning before exact matching.
~~~~

### SRC-COMP-L1543-BA7D73770D5E

- Kind: `deletion`; source: `compiler-proposal.md:1543-1543`; target: `node:SST0`; text SHA-256: `ba7d73770d5e57e03693e1718200f39429286dad6b8a634b31d777aeb5a93e3e`.

~~~~markdown
**Deletion/abort:** delete competing CSS grammar/scanners after parity; stop if authored/processed map basis is incomplete.
~~~~

### SRC-COMP-L1545-F52D711103D5

- Kind: `context`; source: `compiler-proposal.md:1545-1545`; target: `node:SST0`; text SHA-256: `f52d711103d50a437830c6fbcd04fb4bab49a0f82f6d26d1c791c6e8488dd090`.

~~~~markdown
---
~~~~
