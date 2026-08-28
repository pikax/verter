# Exact operative source-clause attachment — FMT2

Schema: 1. Node: `FMT2`. Clause count: 2. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-EXP-L1345-6BD9553F5437

- Kind: `context`; source: `successor-expansion.md:1345-1345`; target: `node:FMT2`; text SHA-256: `6bd9553f54372c327fdd484bc12d2fb106587d9191c4abc480ee0df33ecaab19`.

~~~~markdown
### `FMT2.md` — Native JS/TS/JSX/TSX printers
~~~~

### SRC-EXP-L1347-A70C467A24E1

- Kind: `forbidden`; source: `successor-expansion.md:1347-1352`; target: `node:FMT2`; text SHA-256: `a70c467a24e1b3722d07005c7dec128f597bbac58bf4708beff44cd029260300`.

~~~~markdown
**Intent:** make Verter format embedded script contents itself using the shared frontend facts.
**Predecessors:** `FMT1`, `FCFG0`.
**Subblocks:** (1) JS printer; (2) TypeScript syntax; (3) JSX/TSX; (4) comment/trivia/recovery behavior; (5) range/cursor/edit/maps; (6) Prettier differential plus pinned oxfmt bug-evidence fixtures; (7) performance/allocation profiling.
**Acceptance:** locked `prettier-exact` cells are byte-equivalent and `verter-default` divergences are individually proven; repeated formatting is stable; OXC remains syntax owner but no external formatter runs in production.
**Forbidden:** two option vocabularies, subprocess formatting, unsupported syntax silently unchanged under a success result, or framework rules in base printers.
**Deletion/abort:** abort a compatibility cell rather than fabricate parity; unsupported cells remain truthful.
~~~~
