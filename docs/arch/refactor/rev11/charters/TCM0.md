# TCM0 — Current TypeScript contract and dual-plane architecture lock

**Status:** DRAFT, pending DAG amendment + authorization record.
**Class:** foundational (investigation + architecture lock).
**Predecessors:** the accepted checkpoint established by the ledger ratification.
**Downstream:** TCM1, TCM2, TCM3, TCM4 all depend on this.
**Production routing:** READ-ONLY. TCM0 changes no production route.

## Objective

Establish, from the exact candidate package rather than from documentation, what
the current TypeScript content-mapper and semantic-API contracts actually are,
and lock the dual-plane architecture on top of them.

The two planes are complementary capabilities, not old and new routes:

- **Projection plane** — TypeScript calls Verter's content mapper for generated
  output, span mappings, mapper diagnostics, directives, config and watched-file
  identities, primary and supplemental outputs.
- **Semantic capability plane** — Verter-side feature execution, each feature
  owned by exactly one of `TypeScriptLspDirect`, `VerterWithTypeSemanticOracle`,
  `VerterNative`, or `DisabledByExplicitApprovedContract`.

There is no legal owner named `LegacyProvider`, `CarrierProvider`,
`TsserverFallback`, `RelayFallback` or `CompatibilityProvider`.

## The acyclic invariant this locks

The mapper callback must never query the TypeScript semantic API or send LSP
requests. The only legal order is: TypeScript requests transform → Verter
compiles and returns output plus mappings → TypeScript commits its snapshot →
Verter may then acquire that snapshot → Verter-owned operations may query it.

TCM0 specifies the discriminating deadlock/reentrancy test that proves the cycle
is impossible; TCM2 implements it.

## Scope

1. **Exact package lock.** Inspect the candidate `typescript@7.1.0-dev.20260822.1`
   tarball and binaries. Record package digest, source-commit provenance, exact
   mapper request/response shapes, manifest shape, configured vs inferred project
   behaviour, semantic API availability, LSP API-session behaviour, trust and
   `--runExternalCode` behaviour, declaration/build/watch/incremental behaviour,
   and known defects. **A package published after a merged PR does not
   necessarily contain every repository-main change** — verify, do not infer.
2. **Semantic API certification.** Probe session initialisation, snapshot
   acquisition/update/disposal, project and source-file lookup, `Program` and
   `TypeChecker` operations, bulk symbol/type/reference queries, completions,
   diagnostics, cancellation, and failure behaviour. **Reproduce the known
   stale-snapshot and API-session-hang defects against this exact package.** If a
   required correctness probe fails: do not certify, do not add a relay
   workaround — select a later package or keep TCM4 blocked.
3. **Feature-ownership ledger.** Inventory every `TypeProvider` method, call
   site, capability and background consumer. One row each, recording: current
   implementation, current callers, framework/source region, new primary owner,
   required TypeScript capability, mapping class/mask, diagnostic behaviour,
   failure behaviour, conformance test, performance cell, and what TCM4 deletes.
4. **Diagnostic ownership matrix.** Compiler diagnostics, mapper
   parse/config diagnostics, directives, framework diagnostics, duplicate
   classes, generated-region diagnostics, external-unit diagnostics — with
   deterministic attribution, suppression, precedence and dedup rules. A
   generated diagnostic without a valid authored projection stays visible with
   honest generated attribution; it is never mapped to a convenient false
   position.
5. **Projection-class contract.** Ratify the minimal class set and the terminal
   policy deriving TypeScript feature masks from class × relation × region ×
   owner × certified capability. Every wire span gets an explicit mask — never
   omitted into the upstream all-features default.
6. **External-source decision table.** Each of inline script/template/style, Vue
   custom blocks, Svelte regions, `<script src>`, `<template src>`, external
   styles, imported Svelte assets, supplemental outputs and multi-unit helpers
   gets exactly one model: TypeScript owns it, it is independently content-mapped,
   Verter owns it, or the shape is unsupported and activation fails closed.
7. **Topology benchmarks.** Projection plane: native mapper with in-process
   compiler; thin mapper over a shared native daemon; Node/N-API only if
   competitive. Semantic plane: attach to the editor-owned API session; direct
   native client; managed process for non-editor hosts. Measure cold start, first
   / warm / unchanged transform, rapid edits, CPU, allocations, RSS and peak,
   process count, IPC bytes, open/close, consolidation, crash isolation, cleanup.
   Select the non-dominated topology on evidence.
8. **Cache and lifecycle contracts.** One cache implementation and invalidation
   law per host process. Prepared-artifact keys may include source identity,
   framework/language mode, codegen options, source-unit revisions, product
   profile, projection schema identity, compiler ABI. They must NOT include
   feature-mask policy, `projection_policy_id`, UTF-8 vs UTF-16, wire
   representation, or V3 encoding options.
9. **Deletion closure.** Name every mechanism TCM4 deletes and every generic
   facility that survives with a proven owner. Not deferred to TCM4.
10. **Performance baselines**, locked before any implementation result is seen.

## Non-scope

No production code. No routing change. No mapper process. No package
publication. No activation.

## Acceptance

TCM0 cannot be accepted with any of: "semantic mechanism TBD"; "retain provider
temporarily"; an unclassified `TypeProvider` method; a feature claimed by two
owners; or an intentional capability removal without explicit governance
approval.

## Abort / rescope

Content mapping treated as semantic querying; the candidate package failing a
required semantic correctness probe with no certified successor; a required
feature with no legal owner; or evidence that the acyclic invariant cannot hold.
