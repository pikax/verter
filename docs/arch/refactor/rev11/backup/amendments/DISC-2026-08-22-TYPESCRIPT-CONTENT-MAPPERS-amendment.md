# DISC-2026-08-22 — TypeScript content-mapper amendment train

**Status:** RATIFIED by the maintainer, 2026-08-22. This amendment adds blocks to
the DAG; it does not authorise any of them to execute. Each still requires its own
digest-bound `[[authorization]]` record before dispatch.

**Discovery:** `docs/arch/refactor/rev11/discoveries/DISC-2026-08-22-TYPESCRIPT-CONTENT-MAPPERS.md`
**Classification:** `DISC-ARCH` + `DISC-INVESTIGATE`

## 1. What changed upstream

TypeScript introduced a content-mapper protocol — a projection callback
(`initialize` / `openProject` / `transform` / `closeProject`) through which
TypeScript obtains generated output, span mappings, mapper diagnostics,
directives, config and watched-file identities, and supplemental outputs.
See `microsoft/TypeScript#63800`, `microsoft/typescript-go#4712`,
`microsoft/TypeScript#63936`.

It is **not** a reverse semantic-query interface. It cannot answer hover,
completion, rename or checker questions. Verter's `TypeProvider` surface is far
broader than projection transport, so this is not a like-for-like replacement and
deleting that plane without per-feature owners would lose capability.

## 2. Why this is an architecture amendment and not merely an investigation

`source_projection_map()` returns a JSON **string**, and that string is parsed
back at roughly eight PRODUCTION sites — `provider_surface_store/producers.rs:684`
and `:957`, `server/rename_plan.rs:518`, `documents/mod.rs:652`, `:1068`, `:1161`,
`:1240`, `:1317`, and `server/aux_features.rs:1427`. Separately,
`verter_tsc/src/checker.rs:411` base64-encodes the same string into a
`sourceMappingURL`.

Semantic projection is therefore string-encoded across the live IDE surface, not
merely at one boundary. No correct TypeScript span-map adapter can be built on
that, and work written against the string shape meanwhile is written against a
shape that must change. That migration — TCM1 — is the load-bearing element; the
rest sequences behind it.

## 3. Blocks added

| id | name | predecessors |
|---|---|---|
| TCM0 | Current TypeScript contract and dual-plane architecture lock | A6 |
| TCM1 | Compact mapping products inside `CodeTransform` | TCM0 |
| TCM2 | Content-mapper projection plane (dormant until TCM4) | TCM0, TCM1 |
| TCM3 | TypeScript semantic capability closure (dormant until TCM4) | TCM0, TCM1 |
| TCM4 | Atomic activation and deletion | TCM0, TCM1, TCM2, TCM3 |

Charters: `docs/arch/refactor/rev11/charters/TCM{0,1,2,3,4}.md`.

TCM0 is read-only with respect to production routing. TCM2 and TCM3 remain
unregistered and unreachable from production — no feature flag, no environment
variable, no second provider selection, no fallback — until TCM4.

## 4. The architecture this locks

Two complementary planes under one certified contract, not old and new routes:

- **Projection plane** — TypeScript calls Verter's content mapper.
- **Semantic capability plane** — every user-visible feature owned by exactly one
  of `TypeScriptLspDirect`, `VerterWithTypeSemanticOracle`, `VerterNative`, or
  `DisabledByExplicitApprovedContract`.

There is no legal owner named `LegacyProvider`, `CarrierProvider`,
`TsserverFallback`, `RelayFallback` or `CompatibilityProvider`.

**The acyclic invariant:** the mapper callback must never query the TypeScript
semantic API or send LSP requests. Legal order is transform → compile → return
output and mappings → TypeScript commits its snapshot → Verter may then acquire
and query it. TCM2 carries a discriminating test proving the cycle is impossible.

The four mapping products stay distinct — `PlacementMap`,
`SourceProjectionMap`, `RuntimeSourceMapData`, `EncodedSourceMap`. They may share
packed primitives; they are not collapsed into a universal map.

## 5. What this amendment does NOT do

It does not authorise execution, select a TypeScript package, certify a codec,
delete anything, or change production routing. Version is candidate discovery,
not activation authority: a preview sorts differently from stable and a version
string does not prove the contract is present.

## 6. Ratification

Ratified by the maintainer on 2026-08-22 as the disposition of the registered
discovery. The blocks enter the DAG as `LOCKED` with every identity, evidence and
review field empty, which is their accurate state.
