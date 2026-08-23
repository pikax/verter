# TCM3 — TypeScript semantic capability closure

**Status:** PREPARED — charter structure implementation-ready (5-part shape, numbered exit criteria).
Digest-bound authorization record still required before dispatch; ledger status stays LOCKED until TCM0
is ACCEPTED and TCM1's mapping contract is ACCEPTED. TCM0's own topology/performance-number gaps are
tracked in `evidence/TCM0/OPEN-GAPS.md` and gate TCM0's acceptance, not this charter's readiness.
**Predecessors:** TCM0, TCM1. May run parallel to TCM2 if TCM0's DAG permits (§4 below settles this: yes,
subject to the stated shared-authority caveat). **Downstream:** TCM4. **Dormant until TCM4.**
**Authority:** `rulings/MAINTAINER-STEERING-TCM-CONTENT-MAPPERS.md` §"Semantic capability plane", TCM3
section, global locks §3-4, §10, §13; `evidence/TCM0/feature-ownership-ledger.md` (all 31 rows +
sub-rows, and the TCM3-EC-G1 gate below); `evidence/TCM0/package-lock-and-semantic-api.md` §4;
`evidence/TCM0/diagnostic-ownership-matrix.md`; `evidence/TCM0/distributed-lifecycle-contract.md`;
`rulings/MAINTAINER-RULING-TCM-PACKAGE-CERTIFICATION-SETTLED.md`.

## 1. Intent contract

**Actor / problem.** Every `TypeProvider` capability the content mapper's projection plane cannot express
(component-meta type queries, cross-region rename, carrier-lifecycle notifications, and any feature
needing Verter-specific knowledge the mapped file alone cannot carry) still needs a Verter-side answerer.
TCM3 implements those answerers per TCM0's ratified feature-ownership ledger, replacing today's broad
`TypeProvider` relay abstraction with owner-specific, narrow implementations.

**Required observable outcomes.**
- Every `VerterWithTypeSemanticOracle`-owned row (or sub-row, per `feature-ownership-ledger.md`'s
  2026-08-23 split correction) is answered through the narrow, snapshot-bound, batch-oriented oracle —
  never through a new IPC layer preserving the old `TypeProvider` shape.
- Every `VerterNative`-owned row is answered without querying TypeScript at all.
- A `Program`/`Checker` handle is never held past its owning `Snapshot`'s `dispose()` — enforced
  structurally where the language allows it, per the reproduced §4c defect.
- Rows #25-26 (`register_carrier_member`/`activate_carrier_member`) reach a ruled disposition before this
  block is marked complete (TCM3-EC-G1, §5 below).

**Forbidden observable outcomes.**
- A private legacy TypeScript query protocol under any name.
- A semantic-plane failure fabricating an empty successful result for a feature that needed the failed
  capability (it must fail honestly, though independently-owned direct-LSP features may keep working).
- A second TypeScript project graph in editor-attached operation, unless TCM0's topology benchmark and
  maintainability review explicitly proved it necessary and superior (they did not — TCM0 attaches to the
  editor-owned graph by default).

**Authority / fallback order.** Preference order per steering: (1) direct TypeScript LSP when content
mappings express the feature correctly; (2) Verter native when framework analysis is authoritative; (3)
the narrow official semantic API only when Verter-owned features truly need checker/project facts. There
is no fourth fallback tier.

## 2. Owned scope

1. **The narrow `TypeSemanticOracle`.** A structured, snapshot-bound, batch-oriented, cancellation-aware,
   generation-validated, bounded interface, independent of mapper-process state, incapable of retaining
   remote compiler objects past snapshot release — shape per steering's provisional
   `acquire_snapshot`/`query_batch`/`release_snapshot` trait; the final interface may differ but must keep
   every one of those properties. Prefer bulk operations: symbols/types at several positions, reference
   sets, component/member surface extraction, batched checker facts for one template operation.
2. **`TypeScriptApiSessionState` and `VerterSemanticClientState`** (`distributed-lifecycle-contract.md`) —
   the two local lifecycle owners TCM3 implements. Snapshot acquisition/update/disposal, project/
   source-file lookup, and the oracle-client's own bounded singleflight/project-handle/derived-cache
   state.
3. **The never-outlive-snapshot design constraint**, required by the reproduced §4c defect
   (`package-lock-and-semantic-api.md`: a `Program` handle obtained from a `Snapshot` silently serves
   stale cached content after that `Snapshot`'s disposal, with zero error, while every sibling method
   fails closed correctly). TCM3 enforces this structurally (a type-state/lifetime rule) wherever the
   surrounding language allows it, per this program's stated preference for structural guards over
   runtime discipline — not left to case-by-case caller diligence.
4. **The session-attach topology certification TCM0 explicitly did not run.** TCM0 certified the
   direct-native-client topology candidate live but did NOT probe `API.fromLSPConnection`
   (`custom/initializeAPISession`) for the session-initialization-hang defect class. TCM3 must run that
   probe itself before selecting the attach topology candidate — it does not inherit TCM0's certification
   by association (`tcm1-tcm4-charter-refinements.md`'s TCM3 note).
5. **A designed cancellation strategy.** The certified candidate's API surface has NO cancellation
   primitive (`package-lock-and-semantic-api.md` §4e: exhaustive grep across both sync and async API
   surfaces, zero hits for `cancel`/`Cancel`/`AbortSignal`). TCM3 designs its own in-flight-query
   abandonment (fresh snapshot, not a server-side cancel) — this is NOT an open question TCM3 may leave
   unresolved.
6. **Snapshot correctness**: exact project/session identity and generation; immutable snapshot scope;
   explicit release; no handle reuse; stale-response rejection; cancellation under rapid edits; bounded
   concurrent queries; honest failure states; deterministic recovery using the SAME certified mechanism
   (never switching to the carrier route, per `distributed-lifecycle-contract.md`'s fail-closed rule).
7. **Capability closure over every `VerterWithTypeSemanticOracle`/`VerterNative` row** in
   `feature-ownership-ledger.md` (post-2026-08-23 split correction): for each, a test proving TypeScript
   answers correctly, or Verter answers natively, or Verter answers via the certified oracle — never left
   unproven.
8. **If the official API lacks a required capability**, the legal outcomes are exhaustive: reassign to
   direct-LSP ownership, implement natively in Verter, require and certify an upstream API addition/fix,
   or keep TCM4 blocked. No fifth outcome (silently degrading a feature, or quietly routing it through a
   private protocol) is legal.

## 2a. Timing taxonomy

Every TCM3 timing-sensitive mechanism is classified using `architecture.md` §1.6.

- Snapshot acquire/query/release is **owned causal progress**: generation-validated, snapshot-bound
  receipts. Interest is registered before durable snapshot state is rechecked.
- Bounded concurrent oracle queries consume G2 `FlightCell` (one producer per key and exact snapshot
  basis, joined waiters, independent cancellation and deadlines, durable `ReadyAt`). TCM3 owns snapshot
  scope, cancellation-by-fresh-snapshot (the certified candidate has no server cancel), and
  stale-generation rejection. TCM3 does not ship a second generic flight system, a local duplicate
  `SingleflightGroup`, or a TypeProvider-shaped IPC that coalesces queries beside G2.
- The TypeProvider closure remains TCM3/TCM4. A surviving `TypeProvider` relay that still coalesces
  concurrent demand for the same query identity is a G2/K3 inventory row, not an unnamed TCM3 cell.
- **Recorded precondition gap — not a TCM3 decision, and not resolved here.** The G2 dependency stated
  above and exit criterion 8 both bind TCM3 to G2's `FlightCell` and forbid a local duplicate, but
  `program-dag.toml` gives TCM3 predecessors `["TCM0", "TCM1"]` only. On that DAG, TCM3 can become
  dependency-eligible before G2 exists, and exit criterion 8 is then unsatisfiable by construction:
  TCM3 could only ship the forbidden local duplicate or fail to close. TCM3 does not fix this and no
  block may fix it unilaterally — adding `G2` as a predecessor is a DAG edge change, which requires a
  formal DAG amendment (`AMD-*`) ratified through the architecture-consult route and recorded by the
  program orchestrator, exactly as `rulings/ARCH-RULING-C2-FIVE-FORKS.md` fork B required for
  `B6 -> C2`. The state block set must equal the DAG block set, and `scripts/validate-program-state.mjs`
  enforces that pairing, so an edge added outside that route is a validator violation rather than a fix.
  This bullet records the gap so it is visible at dispatch time; it creates no scheduling rule of its own
  and changes no status.

## 3. Owned-scope boundary (what TCM3 does NOT own)

- No `MapperProcessProjectState` implementation — that is TCM2's.
- No `TypeScriptLspDirect`-owned rows' actual request/response path — TypeScript answers those directly;
  TCM3 must not duplicate or re-remap them (feature-ownership-ledger.md's `a`-suffixed sub-rows).
- No editor activation, no `tsconfig` mutation, no production registration — TCM4 only.
- No re-derivation of the feature-ownership ledger's owner assignments — TCM3 implements TCM0's ratified
  decisions; a genuinely different split discovered during implementation is a rescope trigger routed
  through the program orchestrator, not a silent charter deviation.

## 4. Parallelism with TCM2

The steering permits TCM2 ‖ TCM3 "only if TCM0 and the external DAG permit it," both depending on TCM1's
accepted mapping contract. Settled here: **yes, TCM2 and TCM3 may run concurrently** once TCM0 and TCM1
are both ACCEPTED — they touch disjoint owned-scope surfaces (TCM2: mapper process, projection plane;
TCM3: oracle client, semantic-capability plane) and share no mutable state (`distributed-lifecycle-contract.md`'s
four local owners are two-per-block with no cross-block owner). The one shared surface both blocks READ
(never write) is `projection-class-contract.md`'s ratified class/mask policy and
`feature-ownership-ledger.md`'s ratified row assignments — both already ratified by TCM0, so neither
block blocks the other on it. `program-dag.toml`'s existing `predecessors = ["TCM0", "TCM1"]` for both
blocks (unchanged by this integration, see the DAG section of the integration report) already expresses
this correctly; no edge change is required.

## 5. Numbered exit criteria

1. **`TypeSemanticOracle` implemented with all seven required properties** (owned-scope item 1):
   structured, snapshot-scoped, batch-oriented, cancellation-aware, generation-validated, bounded,
   independent of mapper-process state. Evidence: the trait/type definition plus a test for each property
   (e.g. a bounded-concurrency test, a generation-mismatch rejection test).
2. **Never-outlive-snapshot proof.** Evidence: a structural (type-state/lifetime) test that FAILS to
   COMPILE if a `Program`/`Checker` handle is retained past its `Snapshot`'s `dispose()` call — a
   compile-fail fixture in the same discipline as TCM2's acyclic-invariant proof, not a runtime assertion
   alone.
3. **Session-attach topology probe completed and recorded**, closing TCM0's explicit gap (owned-scope
   item 4). Evidence: a probe report (mirroring `package-lock-and-semantic-api.md`'s §4a-e structure)
   naming whether `API.fromLSPConnection` reproduces the session-initialization-hang defect class, before
   that topology candidate may be selected for editor-attached operation.
4. **Cancellation-strategy design documented and tested.** Evidence: a test proving an abandoned in-flight
   query does not corrupt a subsequent fresh-snapshot query's result (the "fresh snapshot, not server
   cancel" strategy, owned-scope item 5), under a rapid-edit fixture.
5. **TCM3-EC-G1 — rows #25-26 dispositioned.** A maintainer ruling on `feature-ownership-ledger.md` rows
   #25-26 (`register_carrier_member`/`register_carrier_metadata`,
   `activate_carrier_member`/`activate_carrier_members`) is obtained and recorded in
   `docs/arch/architecture-lock/ledger/authority-registry.toml` before TCM3 is marked complete. This is a
   BLOCKING exit criterion, not an optional one — TCM3 cannot close with these two rows still marked
   `CANDIDATE`.
6. **Capability closure test suite** (owned-scope item 7): one test per ledger row/sub-row proving its
   ratified owner answers it correctly; a companion negative test proving no `TypeScriptLspDirect`-owned
   row also produces a duplicate Verter-side result (the "no feature has two primary owners" /
   "no duplicate TypeScript and Verter result" acceptance invariants, applied per-row).
7. **Stale-handle/generation-mismatch rejection tests**: a query against a superseded snapshot generation
   is rejected, not silently served stale data — the general form of the §4c defect, proven fixed at the
   oracle boundary (not merely at the `Program`/`Checker` type-state layer of exit criterion 2).
8. **Bounded-concurrency and cleanup tests**: a fixed ceiling on concurrent outstanding queries; project-
   handle and derived-cache state released on session close, with no leak across repeated open/close
   cycles. Concurrent identical snapshot-bound queries join one G2 `FlightCell`; a test fails if a second
   generic coordinator or a local duplicate `SingleflightGroup` produces that join.
9. **Semantic-plane conformance fixtures pass**, per steering's "Required conformance coverage" list,
   scoped to TCM3's semantic-capability-plane responsibilities: configured/inferred project trust states,
   snapshot correctness across cancellation/regeneration, and the oracle-boundary Unicode/large-file
   classes that reach `TypeSemanticOracle`.

## 6. Forbidden

- Routing all existing `TypeProvider` methods through a new IPC layer merely to preserve their old shape.
- A generic LSP-response reconstruction, markdown parsing, or private carrier protocol anywhere in the
  oracle client.
- Mirroring TypeScript's entire remote object graph into Verter, absent a TCM0 topology-benchmark-and-
  maintainability finding proving that superior (none exists — this is a default-forbidden posture).
- A second TypeScript project graph in editor-attached operation without the same explicit TCM0 proof.
- Fabricating an empty successful result for a feature whose required semantic capability failed.
- Retaining a `Program`/`Checker` handle past its `Snapshot`'s disposal (the exact §4c defect, now a
  forbidden pattern for TCM3's own code, not merely an upstream observation).
- Assuming a server-side cancellation primitive exists (it does not, §4e) — any design that silently
  relies on one is forbidden.
- Deferring the rows #25-26 disposition past this block's own completion (TCM3-EC-G1 is blocking, not
  advisory).
- A second generic flight system, a local duplicate of `FlightCell`/`SingleflightGroup`, or coalescing
  concurrent oracle demand beside G2. Snapshot completion inferred from sleep, idle, or polling when a
  snapshot receipt exists.

## 7. Material bounds

Per `performance-baselines.md`:

1. **Interactive-tier features must not regress versus today's relay-based latency** for the rows TCM3
   now owns — measured against the existing relay baseline before TCM4 may delete the relay code.
2. **Semantic batch-query latency and snapshot-update cost** are measured for the named metrics (edit-to-
   hover, edit-to-completion, edit-to-definition, edit-to-diagnostic, semantic snapshot update, semantic
   batch-query latency) — `performance-baselines.md` locks the hard requirements and one reference point
   today; it does NOT yet carry a numeric threshold for each of these named metrics (tracked as
   `evidence/TCM0/OPEN-GAPS.md` item G-PERF-NUMBERS, gating TCM0's own acceptance). Until that table is
   populated, TCM3's bound is the existing relay-baseline comparison (item 1) — never a number invented
   post-implementation, and never silently waived for the metrics the table has not yet locked.
3. **Zero cancellation-primitive dependency**: the cancellation strategy (owned-scope item 5) must not
   introduce unbounded resource growth under rapid-edit abandonment — bounded per the "bounded concurrent
   queries" snapshot-correctness requirement (owned-scope item 6).
4. **The reproduced §4c defect's design constraint is a correctness bound, not a performance one** — it
   is measured by exit criterion 2's compile-fail proof, not a benchmark; no performance gain from
   accidentally serving stale post-dispose data is ever a legitimate optimization.
5. Package certification is settled (`rulings/MAINTAINER-RULING-TCM-PACKAGE-CERTIFICATION-SETTLED.md`);
   the certified candidate's absence of a cancellation primitive (§4e) is binding on TCM3's design — a
   later certified package that adds cancellation would widen this bound, but TCM3 may not assume that
   package today.

## Abort / rescope

Per steering global abort conditions, applied to TCM3: content mapping is treated as semantic querying;
`TypeProvider` is deleted before its capability ledger closes (or retained after it closes); a private
carrier/query protocol remains; a required feature has no direct-LSP/oracle/native/approved-disabled
owner; remote compiler handles outlive their snapshot; the selected package fails a required semantic-API
correctness probe with no certified successor (package certification is currently settled, but a FUTURE
candidate failing certification re-triggers this condition).
