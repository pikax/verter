# Revision 9 Implementation-Readiness Review

**Verdict:** Revision 9 was architecturally strong but **not ready for unrestricted implementation**. It was safe to begin baseline measurement, characterization, and urgent fail-closed corrections. It was not safe to delegate all foundational cutovers because several cross-owner contracts remained open.

# 1. What Revision 9 got right

Revision 9 correctly established the durable direction:

- a real borrowed direct compiler distinct from the managed engine;
- minimum prerequisite closure and exact live-artifact reuse;
- one semantic authority with deterministic derived projections;
- a concrete sealed compile-semantic facade;
- TypeScript semantic profiles;
- operation-specific DTOs and optional graph export;
- demand-selected flow domains;
- value-side validation and return-only results;
- owner-affine local state and bounded retention;
- terminal rendering, source maps, provenance, serialization, and FFI;
- no permanent dual production paths;
- reproducible performance and long-running memory proof.

Those decisions are preserved in Revision 11.

# 2. Blocking readiness findings

## R9-B1 — Ratification status was contradictory

The manifest and execution documents treated ADRs as architecture authority while the ADRs themselves were marked `Proposed`. An implementor could not know whether a conflicting implementation detail required an ADR amendment.

**Revision 11 correction:** every durable ADR is `Accepted`; the manifest defines exact precedence and amendment rules.

## R9-B2 — The dependency graph was not executable

Revision 9 described a DAG but did not provide a complete machine-readable predecessor relation. Important orderings—input authority before managed query convergence, final flow before TypeExpr cutover, and numeric gates before hot-path implementation—were not unambiguously enforced.

**Revision 11 correction:** `program-dag.toml` defines every block and predecessor; `program.md` explains each edge and abort condition.

## R9-B3 — Flow migration could preserve two production authorities

Revision 9’s sequence built graph/domain/semantic blocks and deleted displaced flow paths only at the end. Accepted intermediate merges could therefore retain the old evaluator as the production authority while a second nearly complete solver existed, or expose both for comparison.

**Revision 11 correction:** `D1` builds only a private hermetic foundation. `D2` is one indivisible public cutover: all public flow operations move to the new solver and the old evaluator, caches, tasks, flags, and guards are deleted. Later blocks expand only the sole solver; unsupported behavior remains typed and non-admissible.

## R9-B4 — Parse identity could encode consumer duplication

A parse `role`/`flavor` dimension could allow IDE and runtime consumers to create distinct parses even when construction semantics were identical. The phrase “one live parse identity” also did not state whether the scope was process-global, per direct invocation, per prepared value, or per managed worker.

**Revision 11 correction:** `ParseKey` contains only syntax-construction dimensions. `ParseOwnerDomainId` separately names direct invocation/batch, `PreparedCarrier`, or managed owner/shard. The invariant is exactly one live `(owner domain, parse key)` result; direct calls do not acquire a hidden process-global cache.

## R9-B5 — Compiler products were not fully compositional

A single artifact enum could not cleanly express simultaneous runtime, IDE, public API, declarations, diagnostics, mapping, and serialization requests without hidden widening or repeated planning.

**Revision 11 correction:** compiler products are canonical typed per-product requests. Each owns only the output and terminal profiles that affect it; required mappings are part of the product, optional presentation/serialization is product-local, and equal subrequests share reusable subplans. Unsupported, duplicate, or irrelevant combinations fail before expensive work.

## R9-B6 — Project-aware compile was not a closed transaction

Revision 9 mentioned `NeedInputs(LoadSet)` but did not bind plans and facts strongly enough to the prepared root, request, profile, projection schema, and input basis. Retry/no-progress/resource limits and stale fact rejection were under-specified.

**Revision 11 correction:** `prepare -> plan -> project -> emit` is an explicit staged protocol. `CompilePlanToken` and facts carry anti-replay bases. Input loading is batched, coherent, bounded, and restarts from a new snapshot rather than splicing observations into an existing attempt.

## R9-B7 — Profile and policy dimensions were conflated

Semantic interpretation, code generation, presentation, serialization, and execution limits have different compatibility and cache consequences. A general policy identity risked both over-keying and reuse of a weaker result as complete.

**Revision 11 correction:** `TypeScriptSemanticProfileId`, `OutputProfileId`, `PresentationProfileId`, `SerializationProfileId`, `ResultContractId`, and waiter-local `ExecutionPolicy` are distinct. Ordinary deadlines, cancellation, priority, and budgets do not enter reusable result identity; exhaustion is partial/failure.

## R9-B8 — Stable IDs and session handles were conflated

Deterministic cross-regime identity and cohort-local continuation handles cannot share one promise. Raw graph/arena IDs are not stable and keeping them stable can pin storage.

**Revision 11 correction:** `StableEntityId` has a canonical deterministic basis; `SessionHandle` is opaque, generation/cohort-bound, and excluded from cross-session equality.

## R9-B9 — Mapping contracts were ambiguous

IDE/provider companions require exact projection mappings as part of their atomic product. Runtime source maps are optional terminal materialization. Treating both as “source maps” makes a zero-work assertion either false or unsafe.

**Revision 11 correction:** `PlacementMap`, required `SourceProjectionMap`, optional `RuntimeSourceMapData`, and terminal `EncodedSourceMap` are separate types and lifecycle contracts.

## R9-B10 — Shared-flight behavior was not a complete state machine

“Waiter-owned” was directionally correct but did not fully close producer ownership, registration, cancellation, priority/budget aggregation, finalization, panic, shutdown, admission, and cross-snapshot joining.

**Revision 11 correction:** the `FlightCell` owns production. Semantic query flights are exact-`InputBasisId` scoped by default; immutable content flights may cross snapshots. Every transition finalizes once, resolves every waiter, and admits only through the owner’s sealed path.

## R9-B11 — Reusable query identity and exact-basis flight identity were not cleanly separated

An exact snapshot/input basis is necessary for safe in-flight joining, but putting the whole basis into reusable cache lookup makes a still-valid candidate undiscoverable after an unrelated edit. Omitting it from a running semantic flight, however, permits joining work whose eventual read set is not yet known.

**Revision 11 correction:** snapshot-independent `QueryIdentity<Q>` discovers bounded candidates, which are then positive/negative-fact validated. `SemanticFlightKey<Q>` adds the exact `InputBasisId` for default in-flight production.

## R9-B12 — Completeness proof construction remained too abstract

A private proof token is insufficient unless the set of required semantic obligations is closed and the finalizer can prove every obligation was discharged under the same graph, demand, profile, and input basis.

**Revision 11 correction:** each operation has a closed static domain registry, an obligation ledger, typed discharge states, deterministic fixed-point completion, and one private finalizer. Unknown obligations cannot be silently ignored or converted to semantic `any`.

## R9-B13 — Binding dependency direction was not locked

Without a binding crate/module direction, the compiler, semantic facade, session, and framework DTOs could create cycles that force public traits, `Any`, `Arc`, locks, and alternate semantic callbacks.

**Revision 11 correction:** identity/contracts and syntax point inward; semantic kernel is dependency-neutral; compiler depends on the sealed semantic facade; managed/session and adapters depend inward only. Build tests reject forbidden edges and cycles.

## R9-B14 — Performance gates could move after implementation began

Revision 9 had good methodology but no mandatory pre-candidate record containing exact numeric cells, machine/corpus identities, and competitor rules. A candidate could influence its own acceptance threshold.

**Revision 11 correction:** `A6` freezes one machine-readable `performance-gates.toml` and Implementation Lock Record before foundational cutovers. Gate relaxation after seeing candidate results is prohibited; benchmark defects require baseline and candidate reruns.


# 3. Final Revision 11 preflight corrections

Before publication, Revision 11 itself received two additional closure fixes:

- Gate 0 is one ordered lineage. `A0` captures the entry checkout; command/harness work and `A3` safety retraction precede `A4` measurement and `A5` final inventories; `A6` accepts one exact post-Gate-0 implementation baseline. Earlier evidence is refreshed when affected by a SHA change.
- compact source units and mapping contracts now land before the borrowed direct compiler, and the source-sized blanking path is deleted in that bounded cutover. The new public direct route therefore never earns acceptance while depending on a knowingly transitional source model.

# 4. Readiness decision

Revision 11 is ready to adopt and execute at Gate 0. It intentionally does **not** authorize broad architecture changes immediately. That is a feature, not incompleteness: the exact repository state, current-owner dispositions, executable commands, compatibility obligations, and numeric performance gates can only be truthfully fixed from the implementation checkout.

After `A6`, a block becomes implementable only when its predecessor set, charter, current-tree closure, tests, numeric gates, deletion set, and independent reviews are complete. This prevents “following the architecture” while inventing unresolved behavior in code.

# 5. Review limitation

This review examined the architecture documents and frozen source evidence but did not execute Verter’s full Rust/TypeScript/NAPI/WASM suites, differential corpus, benchmarks, provider matrix, or long-running memory soak. Revision 11 turns those into explicit implementation gates rather than treating them as already proven.
