# ADR-021 — TypeScript Content-Mapper Dual-Plane Architecture

**Status:** Accepted (TCM0 — read-only investigation and architecture lock; does not authorize TCM1-TCM4
execution, each of which requires its own digest-bound authority-registry record).
**Decision owner:** TCM0, authorized by the maintainer against `charters/TCM0.md` and
`amendments/DISC-2026-08-22-TYPESCRIPT-CONTENT-MAPPERS-amendment.md`.
**Reopen only if:** the certified candidate package is superseded by a later package whose content-mapper
protocol shape or semantic-API behavior genuinely differs from what `evidence/TCM0/
package-lock-and-semantic-api.md` records, or a maintainer ruling dispositions the two
governance-pending ledger rows (`evidence/TCM0/feature-ownership-ledger.md` rows #25-26).

## Context

TypeScript (`typescript-go`/tsgo) has shipped a content-mapper protocol — a projection callback
(confirmed present in the exact candidate package `typescript@7.1.0-dev.20260822.1`, not merely inferred
from upstream PR text: `evidence/TCM0/package-lock-and-semantic-api.md` §3) through which TypeScript
obtains generated output, span mappings, mapper diagnostics, directives, config/watched-file identities,
and supplemental outputs from an external process. It is not a reverse semantic-query interface — it
cannot answer hover/completion/rename/checker questions. Verter's current `TypeProvider` trait
(`crates/verter_type_runtime/src/traits.rs:130`) is far broader than projection transport
(44 methods/capabilities inventoried in `evidence/TCM0/feature-ownership-ledger.md`), so the content
mapper cannot be a like-for-like replacement for it.

`source_projection_map()` returns a JSON string at the assembly boundary today
(`crates/verter_compiler/src/assembly/publish.rs:75-77`), one of at least nine differently-named
`Option<String>` fields carrying the same kind of data across `verter_compiler` and four more across the
`verter_protocol` FFI boundary (`evidence/TCM0/mapping-products-string-surface.md`). No correct
TypeScript span-map adapter can be built directly on a bare string; TCM1's typed `SourceProjectionMap`
must land before TCM2/TCM3 can be genuinely implemented.

## Decision

Two complementary planes, under one certified contract, are locked as the target architecture:

- **Projection plane** — TypeScript calls Verter's content mapper for generated output, span mappings,
  mapper diagnostics, directives, config and watched-file identities, primary and supplemental outputs.
  Implemented by TCM2, dormant (unreachable from production routing — no flag, no env var, no fallback)
  until TCM4.
- **Semantic capability plane** — every user-visible feature is owned by exactly one of
  `TypeScriptLspDirect`, `VerterWithTypeSemanticOracle`, `VerterNative`, or
  `DisabledByExplicitApprovedContract` — the full assignment is `evidence/TCM0/
  feature-ownership-ledger.md` (44 methods across 31 rows, zero unclassified, two rows explicitly marked
  governance-pending rather than silently defaulted). There is no legal owner named `LegacyProvider`,
  `CarrierProvider`, `TsserverFallback`, `RelayFallback`, or `CompatibilityProvider`. Implemented by
  TCM3, dormant until TCM4.

**The acyclic invariant**: the content-mapper callback must never query the TypeScript semantic API or
send LSP requests. Legal order: TypeScript requests transform → Verter compiles and returns output plus
mappings → TypeScript commits its snapshot → Verter may then acquire that snapshot → Verter-owned
operations may query it. The discriminating deadlock/reentrancy test this invariant requires is
SPECIFIED (not implemented) at `evidence/TCM0/acyclic-invariant-test-spec.md`; TCM2 implements it.

**Four mapping products stay distinct**: `PlacementMap`, `SourceProjectionMap`, `RuntimeSourceMapData`,
`EncodedSourceMap` — they may share packed primitives but are never collapsed into a universal map. Full
current-state audit and TCM1 acceptance bar: `evidence/TCM0/mapping-products-string-surface.md`.

**Diagnostic ownership**, **projection-class/feature-mask policy**, **external-source routing**,
**cache/lifecycle contracts**, and **the deletion closure** are locked per `evidence/TCM0/
diagnostic-ownership-matrix.md`, `evidence/TCM0/projection-class-contract.md`, `evidence/TCM0/
external-source-decision-table.md`, `evidence/TCM0/cache-lifecycle-contracts.md`, and `evidence/TCM0/
deletion-closure.md` respectively.

**Two genuine open items are carried forward, not papered over**: (1) the exact literal JSON-RPC method
name spelling for the content-mapper protocol was not isolatable from static analysis of the stripped
native binary (`evidence/TCM0/package-lock-and-semantic-api.md` §3) — TCM2 must close this via a live
protocol trace or source reading before implementation; (2) the `API.fromLSPConnection`/session-attach
topology candidate was not probed for the session-initialization-hang defect class
(`evidence/TCM0/package-lock-and-semantic-api.md` §4a) — TCM3 must close this before certifying that
topology. A genuine, reproduced defect (not the presumed one) IS recorded: a retained `Program` handle
silently serves stale cached data after its owning `Snapshot` is disposed, while every sibling `Program`
method fails closed correctly in the same state (`evidence/TCM0/package-lock-and-semantic-api.md` §4c) —
TCM3 carries a required design constraint (never retain a `Program`/`Checker` handle past its owning
snapshot's dispose) as a direct consequence.

## Consequences

TCM1-TCM4 have a locked, evidence-based target to implement against rather than a documentation-only
description of an upstream feature. The feature-ownership ledger's two governance-pending rows mean TCM4
cannot delete `register_carrier_member`/`register_carrier_metadata`/`activate_carrier_member(s)` without a
further maintainer ruling — this ADR does not grant that ruling itself. No production routing changes as
a result of this ADR; TCM2/TCM3 remain unreachable from production until TCM4, per the amendment's own
constraint.

## Rejected alternatives

- Treating the content mapper as a semantic-query interface and attempting to route all 31
  `TypeProvider` capabilities through it — rejected on direct evidence: the protocol's own `Transform`
  call is one-directional (output + mappings only), confirmed by the full `APIMethodInfo` table quoted
  in `evidence/TCM0/package-lock-and-semantic-api.md` §4.0, which has no content-mapper-initiated query
  method.
- Certifying the candidate package purely from the upstream PR descriptions, without downloading and
  disassembling the actual tarball/native binary — rejected per the charter's own instruction that a
  published package does not necessarily contain every repository-main change; this investigation
  verified the merge is present in the exact candidate bytes (native-binary Go symbol table,
  `evidence/TCM0/package-lock-and-semantic-api.md` §3) rather than inferring it from dates.
- Unilaterally dispositioning the two governance-pending ledger rows as deleted, on the structural
  argument that the content mapper's own identity fields make them redundant — rejected because the
  charter's acceptance clause forbids an intentional capability removal without explicit governance
  approval; TCM0 records the candidate reasoning and defers the ruling.
