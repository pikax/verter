# Exact operative source-clause attachment — FMT1

Schema: 1. Node: `FMT1`. Clause count: 2. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-EXP-L1327-B53306B41688

- Kind: `context`; source: `successor-expansion.md:1327-1327`; target: `node:FMT1`; text SHA-256: `b53306b41688bc94c6fa5ddb1efebcbae116c1b02755f492d5ed2471aae8b902`.

~~~~markdown
### `FMT1.md` — Document algebra, renderer, edits, cursor, and maps
~~~~

### SRC-EXP-L1329-A65691AB3F9F

- Kind: `forbidden`; source: `successor-expansion.md:1329-1334`; target: `node:FMT1`; text SHA-256: `a65691ab3f9f10f5f42938413c4ab1d7ff32a7c8fac8d4db50ab3c46f2d31a71`.

~~~~markdown
**Intent:** build the framework-neutral formatting mechanics with exact authored provenance.
**Predecessors:** `FMT0`.
**Subblocks:** (1) compact `Doc` algebra and group/break/indent semantics; (2) bounded renderer and line-suffix handling; (3) stable format views/trivia/recovery islands; (4) minimal non-overlapping edits; (5) cursor/range expansion; (6) `FormatPositionMap`, idempotence, fuzz, and budget tests.
**Acceptance:** renderer is deterministic, linear/bounded under adversarial docs, idempotent on locked neutral fixtures, and maps every retained authored position exactly; malformed islands preserve bytes according to lock.
**Forbidden:** semantic-AST pretty printing, quadratic group search, action-map reuse, or whole-file replacement when smaller edits are proven.
**Deletion/abort:** delete prototype formatter primitives after migration; abort on unbounded renderer behavior.
~~~~
