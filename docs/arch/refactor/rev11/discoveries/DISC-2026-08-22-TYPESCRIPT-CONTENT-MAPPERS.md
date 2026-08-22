---
discovery_id: "TYPESCRIPT-CONTENT-MAPPERS"
classification: ["DISC-ARCH", "DISC-INVESTIGATE"]
date: "2026-08-22"
date_source: "stated"
status: "REGISTERED — amendment train drafted, not yet authorised"
upstream:
  - "microsoft/TypeScript#63800"
  - "microsoft/typescript-go#4712"
  - "microsoft/TypeScript#63936"
candidate_package: "typescript@7.1.0-dev.20260822.1"
---

# Discovery — TypeScript content mappers

TypeScript has introduced a content-mapper protocol: a PROJECTION callback
(`initialize` / `openProject` / `transform` / `closeProject`) by which TypeScript
obtains generated output, span mappings, mapper diagnostics, directives,
watched-file and config identities, and supplemental outputs.

It is **not** a reverse semantic-query interface. It cannot answer hover,
completion, rename, or checker questions. Verter's current `TypeProvider` surface
is far broader than projection transport, so this cannot be a like-for-like
replacement — deleting that plane without a feature-by-feature owner would lose
capability.

## Why this is DISC-ARCH, not merely DISC-INVESTIGATE

One thing on the current branch is directly load-bearing and is what later work
would be built on top of:

`source_projection_map()` returns a JSON **string** at the assembly boundary —
consumed as `PositionMapper::from_json(... .unwrap_or(""))`
(`crates/verter_lsp/tests/cases/kebab_tag_mapping_full_columns.rs:65`,
`crates/verter_tsc/src/checker.rs:411`). Semantic projection is therefore
string-encoded today. A typed compact `SourceProjectionMap` must replace it
before a correct TypeScript span-map adapter can exist, and every consumer built
against the string form in the meantime is built against a shape that must
change.

That migration is the ASAP element. The rest of the integration is sequenced
behind it.

## Disposition

- No currently executing block is changing the TypeScript/provider/mapping
  authority this discovery touches, so none needs an amendment window and none is
  interrupted.
- Verified at registration: `block/svelte-css-grammar` does not touch
  `code_transform` (0 files), so the CodeTransform work this train needs does not
  collide with it.
- The amendment branches from the accepted checkpoint that the ledger
  ratification block (`block/ledger-ratification`) is currently establishing —
  not from an observed branch SHA.
- **No block of this train is dispatched until it holds a digest-bound
  authorization record.** Dispatching on inferred readiness is exactly how B5
  landed while LOCKED with an unaccepted predecessor
  (`MAINTAINER-RULING-2026-08-22-BV2-B5-J1.md` §5).

## Train (logical names; ledger IDs assigned at authorisation)

| logical | responsibility | may run in parallel |
|---|---|---|
| TCM0 | current-contract + dual-plane architecture lock; read-only | yes — collides with nothing |
| TCM1 | typed compact `SourceProjectionMap` inside `CodeTransform` | after TCM0's mapping contract |
| TCM2 | content-mapper projection plane; dormant | after TCM1 |
| TCM3 | semantic capability closure; dormant | after TCM1, parallel with TCM2 if TCM0 permits |
| TCM4 | atomic activation + deletion | after all |

TCM2 and TCM3 must be unreachable from production routing until TCM4 — no flag,
no env var, no second provider selection, no fallback.

## The invariant that shapes the design

The mapper callback must never query the TypeScript semantic API or send LSP
requests. Legal order is: TypeScript requests transform → Verter compiles and
returns output plus mappings → TypeScript commits its snapshot → Verter may then
acquire that snapshot and query it. A discriminating deadlock/reentrancy test
must prove the cycle is impossible.
