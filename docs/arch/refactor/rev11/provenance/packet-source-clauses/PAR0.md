# Exact operative source-clause attachment — PAR0

Schema: 1. Node: `PAR0`. Clause count: 12. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-COMP-L1917-F56A2F4EDE95

- Kind: `context`; source: `compiler-proposal.md:1917-1917`; target: `node:PAR0`; text SHA-256: `f56a2f4ede9503edbdb49b299a713a68bf1c612c66390bb4f2969888a8cc1318`.

~~~~markdown
## 11.3 `PAR0`
~~~~

### SRC-COMP-L1919-241FF749AA41

- Kind: `context`; source: `compiler-proposal.md:1919-1919`; target: `node:PAR0`; text SHA-256: `241ff749aa418d8da870f3c7c3df91ffecdf78c471a6d1541aa070b14cd466c9`.

~~~~markdown
Add explicit consumption of:
~~~~

### SRC-COMP-L1921-AA2D18D56BE8

- Kind: `context`; source: `compiler-proposal.md:1921-1921`; target: `node:PAR0`; text SHA-256: `aa2d18d56be88c4677ae0153b7a09ef588c982aaf5f5aae0410229d355bf2022`.

~~~~markdown
- source-backed lexical surface and recovery sidecars;
~~~~

### SRC-COMP-L1922-6111E4699743

- Kind: `context`; source: `compiler-proposal.md:1922-1922`; target: `node:PAR0`; text SHA-256: `6111e4699743169f552344f304d244c164ab4559823abd4222b2559f6c7a2a3e`.

~~~~markdown
- parser-owned `ParseAdmission`;
~~~~

### SRC-COMP-L1923-C868D02CC86D

- Kind: `context`; source: `compiler-proposal.md:1923-1923`; target: `node:PAR0`; text SHA-256: `c868d02cc86db5f4161d09a9fbf64fdecfc3a52f2d13f7629d1ced9752dae34a`.

~~~~markdown
- direct strict path permitted to avoid full tooling-sidecar materialization;
~~~~

### SRC-COMP-L1924-070759D93A9C

- Kind: `requirement`; source: `compiler-proposal.md:1924-1924`; target: `node:PAR0`; text SHA-256: `070759d93a9c33817759c9a1d1f46fa1d71781edae44c146080cc49f0ad6314c`.

~~~~markdown
- at most one authoritative parse per exact region/grammar contract;
~~~~

### SRC-COMP-L1925-C55F38C1E111

- Kind: `context`; source: `compiler-proposal.md:1925-1925`; target: `node:PAR0`; text SHA-256: `c55f38c1e111e561a88bfa1aa54608b203f16c9ef2d34e73949e0753c12664a1`.

~~~~markdown
- no redundant whole-source rescans;
~~~~

### SRC-COMP-L1926-CCE66EA99721

- Kind: `context`; source: `compiler-proposal.md:1926-1926`; target: `node:PAR0`; text SHA-256: `cce66ea9972116aafaa8268e85647b6967b02a03941303f66910091a2db4f5fd`.

~~~~markdown
- raw authored text source-backed;
~~~~

### SRC-COMP-L1927-3E643FBA36DA

- Kind: `context`; source: `compiler-proposal.md:1927-1927`; target: `node:PAR0`; text SHA-256: `3e643fba36da60c4ca79427d1fdd3176fbb8bc420be21d99709e447ece301097`.

~~~~markdown
- dense syntax IDs separate from authored offsets and cross-revision lineage.
~~~~

### SRC-COMP-L1929-E549198EAA49

- Kind: `forbidden`; source: `compiler-proposal.md:1929-1929`; target: `node:PAR0`; text SHA-256: `e549198eaa4975376efeacc97c8720a1b30399123a1abd9fc6a8ceb4d115338e`.

~~~~markdown
`PAR0` must not own `SemanticAdmission` or `CompileAdmission`.
~~~~

### SRC-EXP-L815-8126F7EC831B

- Kind: `context`; source: `successor-expansion.md:815-815`; target: `node:PAR0`; text SHA-256: `8126f7ec831ba5d945fc11937e1bd3302de24ea78a4fe60303cdaf23cfe5e2d2`.

~~~~markdown
### `PAR0.md` — Parser decision, ownership, reuse, and lineage contract
~~~~

### SRC-EXP-L817-05C7B3DA3F90

- Kind: `forbidden`; source: `successor-expansion.md:817-822`; target: `node:PAR0`; text SHA-256: `05c7b3da3f90122e451eab25c9d18c74bd992a1efad199c4c822fb0edfbd13b5`.

~~~~markdown
**Intent:** make parser choice evidence-based per carrier while preventing both arbitrary parser proliferation and an omni parser.
**Predecessors:** `CPF1`, `VID0`.
**Subblocks:** (1) define `ParserDecision`; (2) key ownership by carrier profile + grammar epoch; (3) define safe reuse equality and cache keys; (4) define fork lineage/license/corpus recording; (5) define lossless recovery, error, fuzz, and budget obligations; (6) reserve evidence-gated HTML-family extraction.
**Acceptance:** negative fixtures reject content-hash-only reuse, TSX parser copies, framework switches in a neutral parser, and a tooling-only carrier forced through a compiler backend.
**Forbidden:** global parser family authority, “HTML-like” as a cache key, shared recovery semantics without proof, or parser selection from an unresolved framework name.
**Deletion/abort:** delete any central grammar match made obsolete by owner-local registration; rescope a vertical when its closest parser fails the pinned grammar/recovery corpus.
~~~~
