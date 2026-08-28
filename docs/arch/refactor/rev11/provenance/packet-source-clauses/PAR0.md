# Exact operative source-clause attachment — PAR0

Schema: 1. Node: `PAR0`. Clause count: 16. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

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

### SRC-EXISTING-NODE-AMENDMENT-PAR0

- Kind: `requirement`; source: `existing-node-amendments.md:23-31`; target: `node:PAR0`; text SHA-256: `72c9e48ef14e7ada2a1424e02ecc03aff1e69cc287b841541b7729ca8f2b95c8`.

~~~~markdown
## PAR0 — Parser ownership, reuse, and lineage contract

Add:

- `RecoverySnapshot` and per-region `RecoveryParticipation` are parser/lowering products, not resolver state.
- executable-region discovery is performed during the one parse/shallow pass per content hash;
- no checker or language-service operation may reparse source to recover semantic facts;
- JSDoc is the only approved dedicated type-text parse path, scoped to its parser owner;
- parser errors carry exact extracted-region-to-authored-source lineage.
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

### SRC-LEGACY-LSO-RECOVERY-001

- Kind: `requirement`; source: `legacy-arch-reconciliation.md:88-94`; target: `node:B2`; text SHA-256: `8fff374562fa12bdeb1f432936bbf1db6e42335ae27c5908f4c99050d9f1308b`.

~~~~markdown
### LSO-RECOVERY-001 — Two-rail tolerant recovery

- Recoverable syntax errors publish authored native diagnostics while stable semantic operations continue.
- Recovery preserves authored identifier identity and mapping, inserts minimal capability-tagged synthetic structure, and fails open for incomplete usage analysis.
- Strict mapping remains strict; synthetic diagnostics are suppressed structurally or dropped, never heuristically re-anchored.
- Targets: amendments to `B2`/`PAR0`, `LSO1`, `LSO9`.
- Source: `docs/arch/ide-error-recovery-design.md`, blob `361a41390ffce6772616bffc958fd79f5f8a2ad9`.
~~~~

### SRC-LEGACY-TRANSFER-361A41390FFC

- Kind: `requirement`; source: `legacy-architecture-transfers.md:264-269`; target: `node:B2`; text SHA-256: `80f29abe9dda5dc8d8cd1971421535c511266b66443c065499ca620123b07df7`.

~~~~markdown
### LEGACY-TRANSFER-361A41390FFC

- Original path: `docs/arch/ide-error-recovery-design.md`; Git blob: `361a41390ffce6772616bffc958fd79f5f8a2ad9`; exact source SHA-256: `765426542586b7a3d2dc965f4b396178c70b9504a597425aea2f1e1cd0195938`.
- Exact retained source: `sources/legacy-architecture-transfers/ide-error-recovery-design.md`.
- Applicable authority: `B2`, `PAR0`, `LSO1`, `LSO9`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~

### SRC-LEGACY-TRANSFER-D73EB9761394

- Kind: `requirement`; source: `legacy-architecture-transfers.md:425-430`; target: `node:PAR0`; text SHA-256: `97fe7a1605b4252e31096dff095bce6fa0ea41018e94a7c0d4d5048cda77eaa1`.

~~~~markdown
### LEGACY-TRANSFER-D73EB9761394

- Original path: `docs/arch/parselower-design.md`; Git blob: `d73eb9761394d60a8dc81b3ce334d9aa7bf0c5c3`; exact source SHA-256: `9f4c0dacfa4aa3c7d9b7a25944eacca964d1968324f4c12d33288c574e8da3d4`.
- Exact retained source: `sources/legacy-architecture-transfers/parselower-design.md`.
- Applicable authority: `PAR0`, `B2`.
- Binding: every durable requirement in the exact retained source remains operative for the applicable authority. Text that explicitly records implementation archaeology, a rejected alternative, or a superseded observation is non-operative.
~~~~
