# Verter Architecture Convergence Program — Revision 11

**Status:** Normative dependency-ordered implementation authority.  
**Durable authority:** `architecture.md`, contracts, and accepted ADRs.  
**Proof authority:** `verification.md`.  
**Governance authority:** `governance.md`.  
**Machine-readable predecessor authority:** `program-dag.toml`.

# 1. Program law

The program is a DAG of bounded clean cutovers. A block may start when every predecessor has its required accepted state and the validated `program-state.toml` marks it `READY`/`BLOCK_READY`. A dependent upper layer may instead perform contingent `READY`/`IN_PROGRESS`/`REVIEW` work only when each unaccepted predecessor is a lower layer in the same validated immutable stack snapshot. It cannot become `ACCEPTANCE_RECOMMENDED` or `ACCEPTED` until those predecessors are formally satisfied and the upper candidate is restacked/revalidated on the accepted base. Except for the explicit `D1` private checkpoint, accepted block state means the predecessor is integrated on the intended lineage. `D1` is reviewed on the bounded `D2` integration branch and is never merged/released independently; `D2` is the atomic landing unit. Parallel work is legal only when cutover closures, writable worktrees, and shared generated/owner surfaces are disjoint or explicitly serialized.

A pull-request stack is transport only. Bounded stack windows may linearize one short dependency path for review, but they do not create semantic predecessor authority. A program-wide mega-stack is prohibited. Atomic private layers land only through their final atomic candidate.

Before `A6`, only Gate 0 work is legal. `A2C` is not executable. `A3` may change behavior solely to retract a non-G10 A2-catalogued wrong-complete result through the existing typed degradation/non-admission rails; it has no structural-completion or G10 obligation. Exact structural completion and G10 discrimination remain debt `FR-D8`, owned by D6. No Gate 0 block may create a syntax-only completion detector, second graph/classifier, or false refusal of a checker-correct clean/warm result.

# 2. Critical path

```text
A0–A6  implementation lock
   ↓
B      deterministic direct compiler and shared syntax ownership
   ├─────────────┐
   ↓             ↓
C semantic core  J CSS convergence (bounded parallel after shared identities)
   ↓
D atomic sole-flow-solver cutover and semantic completion
   ├──────────────┐
   ↓              ↓
E public/type     F committed input and load basis
cutovers           ↓
                 G QueryRuntime/flights/executor/cache convergence
                  ↓
                 H managed incrementality/providers/publication
   └──────────────┴──────────────┐
                                  ↓
K framework catalog, typed carriers, host decomposition
                                  ↓
L soak, final performance, optional tuning, final lock
```

# 3. Universal block admission

A non-local block is `BLOCK_READY` only when:

- the `A6` Implementation Lock Record is accepted;
- the program-state ledger validates and marks the block `READY`;
- every predecessor is accepted on recorded SHA/tree identities, or the block is explicitly contingent draft/review work over a lower layer in the same validated stack snapshot;
- touched current-owner rows are resolved;
- consumers/readers/writers/lifetimes are complete;
- identities, profiles, compatibility domains, and failure behavior are exact;
- tests discriminate the invariant and negative cases;
- applicable numeric performance/memory/work gates are populated;
- the one-path cutover and exact deletion set are named;
- abort/rescope conditions and independent reviews are assigned;
- one immutable context packet, writable worktree/branch, and stack-window/layer disposition are recorded.

## 3.1 Program-wide timing law

Every block classifies a timing-sensitive mechanism as **owned causal progress**, **semantic time**, **external liveness**, or **performance measurement**, using `architecture.md` §1.6. Internal correctness cannot use fixed sleeps, repeated `yield_now`, atomic/refcount polling, counters unchanged for a duration, elapsed-time assertions, global-idle heuristics, or time-substituting retry loops. External polling is admitted only with a written proof that no event, receipt, OS primitive, or callback exists and with one real outer watchdog.

No block may introduce a global clock trait, global event bus, universal workspace-generation cache key, global idle/readiness service, global duration registry, production-state-machine event log, or generic coordinator duplicating G2's `FlightCell`.

This four-part taxonomy binds every DAG block that owns a timing-sensitive surface, including the TypeScript content-mapper train (`TCM0`–`TCM4`). TCM0 is lock-only and TCM1 is mapping-product geometry; neither owns a live coalescer, queue, or protocol-admission surface, and they receive no extra concurrency criteria. TCM2's JSON-RPC/stdio mapper boundary is external liveness (protocol completion plus one independent real monotonic watchdog) and TCM2 is the sole owner of mapper-protocol admission inside cancellation and one absolute deadline; G3 may supply a reusable bounded-admission primitive and must not implement that path. TCM3's snapshot-bound oracle is owned causal progress; its bounded concurrent queries consume G2 `FlightCell` and must not ship a second generic coordinator. TCM4's atomic activation is owned causal progress of the cutover; the Project-Bound External-TS rule remains in force and TCM4's performance obligation is not waived. Same-key coalescers are inventoried BY NAME in `charters/K3.md` and each named cell is dispositioned with its final owner; an unnamed cell is NOT a close failure and no search is required to prove absence (`rulings/MAINTAINER-RULING-COALESCER-CLOSURE-IS-NAMED-DISPOSITION.md`: K3 closes when everything named is dispositioned, and we do not keep testing for unnamed cells). The search recorded in that charter is evidence of how the inventory was built, not a gate — which is also what `CLAUDE.md`'s structural-enforcement rule requires.

## 3.2 Bounded `block/deterministic-tests` scope

`block/deterministic-tests` is a prerequisite correction branch, not a new DAG block and not the G/H runtime redesign. It is limited to:

- deterministic test infrastructure;
- exact timer tests;
- direct lost-wake and lifecycle corrections discovered while removing timing assumptions;
- production/test topology alignment needed to test those mechanisms;
- removal of newly added or directly touched polling and elapsed-time correctness checks.

It must not add a permanent readiness coordinator or generic flight system. Those responsibilities remain G2/G3/G5/H2/H3. A narrow urgent latency bridge may land before those blocks only when all seven conditions hold:

- it remains private and narrow;
- it uses an existing producer-owned applied-generation receipt;
- it is keyed by an exact `DocumentRevisionId` or typed basis;
- it permits foreground promotion;
- each waiter has independent cancellation;
- it publishes only while the basis remains current;
- it names the exact G2 or H3 block that will delete it, with K3 responsible for final residual-deletion verification, before landing.

A bridge is not approved merely to improve current benchmark results. Public `LspConfig` switches `suppress_edit_debounced_import_publication` and `suppress_sync_coordinator_signal` are not timing architecture; they create a non-production topology and must not land as production configuration. If they exist on the branch, they name H3 as deletion owner before landing; K3 verifies residual deletion. Gate-performance work stays a separate, independently measurable landing unit; deterministic correctness cannot depend on that experiment landing. Its evidence includes single-gate idle and concurrent-gate contention measurements on the target M3/24GB machine; p50/p95/p99 wall time per lane; CPU, memory, queue/blocked time, and cache misses where available; timeout/retry/flake rate; critical-path attribution; per-lane stall detection; and narrow resource classes instead of unnecessarily broad serialization. Removing sleeps alone is not evidence of a material gate reduction, and external watchdogs are not lowered until measurements establish safe bounds.

# 4. Gate 0 — Implementation Lock (`A0`–`A6`)

## A0 — Adopt Revision 11 and freeze the exact checkout

**Predecessors:** none.  
**Class:** Foundational governance/evidence.

Deliver the completed entry-checkout lock, repository/lockfile/toolchain state, architecture-affecting open-change disposition, Revision 11 package/DAG digests, designated maintainer/orchestrator identities, available GitHub/CI/branch-protection/merge-queue/stack/signing permissions, worktree inventory, and the initial validated `program-state.toml`.

**Exit:** one exact entry SHA/tree and lineage; no floating or equivalence claim; only legal next Gate 0 work is exposed in program state. `A6`, not `A0`, accepts the implementation baseline after Gate 0 source changes.

## A1 — Prove non-vacuous commands and capability truth

**Predecessors:** `A0`.

Prove canonical Rust, TypeScript, NAPI, WASM, corpus, provider, and conformance commands execute intended targets and non-zero work. Complete affected capability-matrix rows and preserve raw evidence.

**Exit:** a green command cannot be empty, filtered to the wrong target, or supported by an undeclared experimental route.

## A2 — Strengthen the U6 public cold/warm differential harness

**Predecessors:** `A0`, `A1`.

Add exact recursive expectations, negative controls, oracle/profile stamps, crossed closure/effect/completion positions, and public replay/admission assertions.

**Exit:** known defect rows discriminate the actual semantic difference and cache behavior.

## A2C — Retired completion predecessor

**Predecessors:** `A2`.

A2C is not executable. Its reachable DAG row is retained as terminal historical state with status `SUPERSEDED`. The rejected completion candidates and incomplete implementation remain unlanded historical evidence and transfer no correctness, performance, mutation, or test result.

**Exit:** none. Exact structural completion and G10 discrimination are debt `FR-D8`, owned by D6 / `U6.LOOP_CLOSURE`; heavy work resumes only after D6 has a closed, code-first carrier inventory. The demanded `FunctionFlowGraph` remains the sole completion reducer, and no syntax-only classifier or second completion authority may be introduced.

## A3 — Retract known wrong-complete results

**Predecessors:** `A2`.

Every non-G10 A2-catalogued wrong-complete path returns a typed degraded usable result or existing typed `NoValue` and is not warm-admitted. Authored or otherwise legitimate semantic `any` remains distinct, and every member of the 154-row checker-correct clean/warm preservation cohort remains complete and warm. A3 has no G10 or structural-completion obligation and must not inspect completion syntax, skeleton regions, events, graph edges, or an endpoint accessor.

**Exit:** every A2-catalogued known wrong-and-warm result except G10 returns a typed degraded usable result or typed `NoValue` and is refused warm admission; authored `any`, the 154-row preservation cohort, and X05/X68/X80/X88 remain checker-correct, clean, and warm. No syntax-only G10 detector exists. G10 remains open debt `FR-D8`, owned by D6.

## A4 — Install measurement-only attribution and capture the baseline

**Predecessors:** `A3`.

Install measurement-only attribution on the post-safety Gate 0 lineage, then count normalization, hashing, parses/reparses, preparation, planning, projection, resolver/relation/inference/flow, CSS work, rendering/maps/provenance/serialization/FFI, copies, allocations, arena live/capacity, tasks/flights/queues, admission/eviction, retained bytes, pins, and deterministic digests by logical identity.

Instrumentation is leaf-only, bounded, and disabled-overhead measured. It does not become semantic authority.

**Exit:** every strategic baseline operation can explain why expensive work ran and how often.

## A5 — Complete current-owner, identity, profile, compatibility, protocol, and consumer inventories

**Predecessors:** `A4`.

Resolve current authorities and every affected direct consumer against the exact post-safety, instrumented lineage; classify all versions/domains and configuration fields; enumerate TypeExpr/component-meta/graph/wire consumers; lock dependency-direction test strategy; reconcile open changes and surviving instrumentation owners; decide evidence paths, program-state custody, worktree/branch naming, CI/merge constraints, review contexts, and bounded stack-window policy.

**Exit:** later blocks cannot create a second owner or discover a hidden public/wire consumer mid-cutover by omission.

## A6 — Accept the Implementation Lock Record

**Predecessors:** `A5`.

Freeze:

- exact entry checkout, exact post-Gate-0 implementation baseline, Revision 11 manifest/DAG/program-state digests, and designated maintainer/orchestrator;
- command/capability and GitHub/CI/permission evidence;
- owner/consumer dispositions;
- identity/profile/compatibility/protocol decisions;
- baseline work/performance/memory data;
- machine-readable `performance-gates.toml` with no placeholders;
- accepted program-state, context-packet, evidence-custody, worktree, branch, CI, merge, and bounded stack-window policy;
- first unlocked block charters, stack placement, writable owners, and review assignments.

Gate thresholds cannot be relaxed after candidate direction is observed. A benchmark defect requires baseline and candidate reruns.

**Exit:** program state becomes `PROGRAM_LOCKED`; foundational blocks may become `BLOCK_READY`.

# 5. Track B — Deterministic direct compiler and shared syntax ownership

## B1 — Neutral contracts, typed identities, profile schemas, and dependency firewall

**Predecessors:** `A6`.

Land the distinct identity/profile/mapping/result-contract types and forbidden dependency-edge build tests. Do not add service owners merely to hold types.

**Exit:** every artifact/query can state construction and compatibility identity without global revision, request identity, or ad hoc strings.

## B2 — Shared syntax frontends and parse owner domains

**Predecessors:** `B1`.

Implement `ParseKey`, `ParseOwnerDomainId`, one live pair, owner-affine access, exact locators, pressure reparse, and direct/prepared/managed lifetime rules. Delete consumer-role parser duplication within each completed cutover.

**Exit:** same syntax construction in one owner domain parses once; direct calls remain free of hidden managed/global cache state.

## B3 — Compositional product planner

**Predecessors:** `B1`.

Implement canonical typed per-product requests, product-local output/terminal profiles, framework requests, minimal prerequisite closure, independently keyed reusable subplans, one-plan-per-request default, unsupported/duplicate-combination rejection, and zero-work counters.

**Exit:** requested and forbidden incidental work is mechanically observable.

## B4 — Compact source units, mapping taxonomy, and blanking deletion

**Predecessors:** `B2`, `B3`.
**Atomicity:** this block does not land while any production compiler route still requires full-carrier whitespace blanking for the migrated source-unit family.

Introduce logical units, `PlacementMap`, required `SourceProjectionMap`, optional `RuntimeSourceMapData`, terminal `EncodedSourceMap`, and immutable generated-code-plus-required-map artifacts. Migrate every current compiler consumer in the bounded family and delete source-sized `eval_source`/blank-buffer ownership in the same accepted cutover.

**Exit:** offset preservation uses exact units/maps rather than a source-sized duplicate, and required IDE maps cannot be skipped by a runtime-map flag.

## B5 — Borrowed one-shot compiler atomic cutover

**Predecessors:** `B4`.

Introduce the borrowed direct boundary over the surviving shared frontend, planner, units, mappings, and framework algorithms. Keep arenas/local state owner-affine, construct no managed/session/provider/audit infrastructure, materialize only requested terminal artifacts, separate pure Rust and FFI paths, migrate every current direct/standalone caller, and delete the displaced public/direct route in the same accepted cutover.

**Exit:** source-only one-shot compile is one genuine low-overhead library product with no shadow direct implementation.

## B6 — `PreparedCarrier`, direct batch, and direct-core closure

**Predecessors:** `B5`.

Add explicit borrowed/owned preparation, prepare-once/emit-many, inspectable retained weight, safe drop, direct batch ownership, deterministic aggregation, and no implicit project/provider/global state. Prove all three direct regimes use the same accepted parser/planner/unit/mapping/codegen core and delete any transition-only adapters.

**Exit:** reuse is explicit and lock-free/single-owner by default; the direct core is final enough for semantic projection and managed orchestration to depend on it.

# 6. Track C — One non-flow semantic core and closed compile projections

## C1 — Converge `ModuleResolverCore` and non-flow `TypeInfoCore`

**Predecessors:** `A6`, `B1`, `B2`.

Preserve correct current resolution/index/fact foundations, remove alternate resolver semantics, use immutable observation views, and return batched `NeedInputs`. Flow semantics are excluded.

**Exit:** lifecycle differs; module/name/type/relation meaning does not.

## C2 — Staged compile transaction and concrete sealed facade

**Predecessors:** `B3`, `B6`, `C1`.

Implement prepare/plan/project/emit, anti-replay plan/fact tokens, concrete `CompileTypeInfo`, first-party lifecycle modes, no public semantic trait object, no blanket `Send + Sync`, and bounded load/retry behavior.

**Exit:** project-aware compilation cannot accept another semantic engine or stale/mismatched facts.

## C3 — Closed Vue runtime macro projection

**Predecessors:** `C2`.

Batch exact demands, deduplicate shared roots, stop at broad constructor terminals, follow imports/generics/unions/mapped/indexed forms only until the closed answer, and return typed degradation/dependencies/profile.

**Exit:** codegen receives only facts it consumes.

## C4 — Zero-work, batch-shape, and regime-equivalence proof

**Predecessors:** `B5`, `C3`.

Prove no-demand zero initialization, Svelte zero Vue/native projection, aggregate-project-once batch shape, and equal direct/prepared/managed semantics.

**Exit:** optional semantics are physically absent when not requested.

# 7. Track D — Atomic sole effective-flow solver

## D1 — Private sole-solver foundation

**Predecessors:** `A3`, `B1`, `B2`, `C1`.

On the bounded `D2` integration branch, behind a hermetic non-production test boundary only, implement canonical graph adapters, deterministic derived projections, static domain registry, demand plan, stable binding slots, integration with the shared relation/inference authority, obligation ledger, private complete finalizer, exact parse reacquisition, and typed gaps. The private replacement contains no name-keyed or flow-private relation authority.

No product entry point or selectable runtime flag may reach this foundation. `D1` may receive checkpoint review approval but must not merge or release independently from `D2`. Before `D2`, it must satisfy every effective-flow capability row declared Supported/Stable in the `A6` matrix, or that row must receive an explicit reviewed breaking maturity/compatibility decision. Experimental/unsupported rows may remain typed gaps according to their contract.

**Exit:** the minimum replacement preserves every ratified supported capability, safely answers its covered cases, and fails unsupported cases without a second production authority.

## D2 — Atomic public flow cutover and legacy deletion

**Predecessors:** `D1`.

In one accepted candidate:

- route every effective-flow product operation to the new solver;
- delete the syntax-shaped evaluator, name-keyed state, duplicate control/completion/relation/effect paths, caches, tasks, metrics, flags, guards, and migration comments;
- keep unsupported mechanisms as typed non-admissible gaps;
- preserve every `A6`-ratified Supported/Stable effective-flow capability and its public failure contract;
- prove one production solver by source, dependency, runtime, and cache-admission tests.

**Exit:** one graph authority and one production solver. This block is indivisible.

## D3 — Shared relation authority and binding/product domains

**Predecessors:** `D2`, `C1`.

Extend the already-shared relation authority with nominal identities including unique symbols and tri-state comparability; complete stable binding slots, one transfer/join per domain, deterministic worklist, and connected budgets. No flow-private relation/name authority may exist after `D2`.

## D4 — Narrowing and structural returns

**Predecessors:** `D3`.

Implement supported narrowing/invalidation/predicate/substitution mechanisms. Every authored return contributes structurally; endpoint completion alone controls fallthrough `undefined`.

## D5 — Closure, capture, freshness, and position-independent effects

**Predecessors:** `D3`.

Retain direct/transitive reads and writes, escape summaries, creation-time frontier/freshness, profile-compatible invalidation, and one effect mechanism across expression positions.

## D6 — Loop fixed points and completion graph

**Predecessors:** `D3`.

Close debt `FR-D8` by deriving exact structural completion and G10 discrimination on the sole demanded `FunctionFlowGraph`, then add deterministic selected-frontier loop convergence, loop-summary edges, state routing, and final clean semantics on that same graph. Heavy completion work may begin only after the D6 lock contains a closed, code-first carrier inventory. Do not reconstruct completion meaning from syntax or create another graph/classifier.

## D7 — `this`, sequence, call/context, and value surfaces

**Predecessors:** `D4`, `D5`, `D6`, `C1`.

Route exposed carriers, sequence pass-through, call/contextual callback, and value-inference interactions through the one kernel. Unsupported async/generator/cross-file/opaque-call areas remain typed gaps.

## D8 — U6 convergence and complete-result/admission proof

**Predecessors:** `D4`, `D5`, `D6`, `D7`.

Make all claimed rows match the pinned semantic profile or return the declared typed gap; prove every ledger obligation/fixed point and warm-admission negative case.

**Exit:** wrong-and-warm is structurally unrepresentable for the claimed operations.

# 8. Track E — Public TypeInfo, TypeExpr, and semantic storage cutovers

## E1 — Complete TypeExpr/component-meta/graph/protocol consumer closure

**Predecessors:** `A5`, `C1`, `D2`.

Turn the seed inventory into an exact producer/consumer/protocol/lifetime map. Name every NAPI/WASM/wire/cache/test route and compatibility obligation.

## E2 — Eliminate internal general `TypeExpr` transit

**Predecessors:** `C4`, `D8`, `E1`.

Migrate consumers to borrowed authored nodes, compact exact chunks, semantic values, or operation DTOs. Delete each displaced producer/conversion/cache in the same bounded cutover; do not replace it with a mandatory general graph.

## E3 — Public operation DTOs and optional bounded graph export

**Predecessors:** `E2`.

Make query-specific DTOs primary; separate `StableEntityId` and `SessionHandle`; retain graph export only for real consumers under a named compatibility/size/canonical-ordering contract.

## E4 — Reclaimable semantic storage and scoped interning

**Predecessors:** `E2`, `E3`.

Implement generation/cohort-safe handles, explicit pins, selective promotion, scoped/compactable interners, and owner-local byte bounds. No public output pins internal arenas.

# 9. Track F — Committed inputs and resumable loading

## F1 — One committed input and coherent snapshot authority

**Predecessors:** `A6`, `B6`, `C4`, `D2`.

Reconcile current VFS/registered-source/workspace publication into one authority; use short immutable-root commits; keep parse/compile/provider work outside the write critical section; preserve ordered document mutation. The committed snapshot exposes typed exact immutable dimensions for document revision, relevant source-root/project revision, and configuration/resolver epoch. It provides the capture seam for a provider/program-applied generation and dependency read-set/stamp without collapsing them into one workspace counter.

**Timing/basis exit:** `EngineRevision` remains a commit-order/capture aid, never a universal invalidation or cache key. An edit changes only the bases it causally affects; unrelated edits do not invalidate unrelated facts.

## F2 — InputBasis, load waves, negative facts, and retry

**Predecessors:** `F1`, `C2`.

Implement exact `InputBasisId`, normalized batched `LoadSet`, stable positive/negative observations, conditional coherent commits, no-progress and resource bounds, and clean preloaded equivalence. Every derived fact records, as applicable, exact document revision, relevant source-root/project revision, configuration/resolver epoch, provider/program-applied generation, and dependency read-set or stamp. Provider generation is accepted only as a producer-owned exact receipt; F2 does not invent provider readiness.

**Timing/basis exit:** value-side validation compares the typed relevant dimensions, not a universal `EngineRevision` or workspace generation. Unrelated edits preserve reusable unrelated facts.

# 10. Track G — QueryRuntime, FlightCell, executor, and cache convergence

## G1 — Hermetic query facts and result contracts

**Predecessors:** `C4`, `D8`, `F2`.

Implement snapshot-independent `QueryIdentity`, exact-basis `SemanticFlightKey`, bounded multi-candidate lookup, positive/negative read facts, exact result contract, owner-controlled compute scope, sealed publish/return-only decision, value-side validation, and operation-specific cycle/failure policy.

## G2 — FlightCell-owned same-key production

**Predecessors:** `G1`.

Implement content and semantic flight classes, exact input-basis scoping, independent waiters, policy aggregation, cancellation, panic/shutdown/finalization, follower validation, and no leader-owned lifetime.

**Acceptance:** `FlightCell` owns one useful producer flight per key and exact basis; producer lifetime is independent of any request handler; multiple waiters join; foreground demand promotes an existing background flight; cancelling one waiter does not cancel production still useful to other waiters; each waiter has an independent absolute deadline; that deadline covers queue admission, execution, and response; completion durably publishes `ReadyAt { basis, artifact/read-set stamp }`; waiters subscribe before rechecking durable state; and obsolete results are rejected before publication. `FlightCell` does not serialize heterogeneous lifecycle transitions or coalesce across bases. G2 absorbs exact-basis production from `ImportSyncMemo` flight mechanics, `external_ts_sync::{QueryDedupeRegistry, QueryAdmission, InflightSlot}` (not `cache_runtime` `InflightSlot`), `CarrierPublicationStore::{lanes, PublicationLane}`, `MetaRuntimeImpl.pendingEngines`, and, if retained, the hover-provenance producer. For IDE-repair, `ResyncCoordinator`, `TsserverCarrierRefresh`, shared-tsgo `CarrierSyncState`/`PendingSubmission`, `DeclOverlayOwner` serialization, `LazyTransport` establishment, `ProjectTsserverProvider`/`LazyManagedTypeProvider` activation, and membership-recovery cooldown, G2 absorbs only exact-basis waiter-join production. H2 owns and deletes the entire IDE-repair mixed cutover (`ide_sync_repair_locks`, `IdeSyncRepairLease`, `IdeSyncRepairLane`, generation ABA helpers) plus serialized `ProviderHub` lifecycle and cross-basis protocol coalescing; H3 owns when LSP demand invokes those mechanisms. G2 deletes displaced exact-basis flight fields as they migrate; H2 deletes the displaced lifecycle/coalescing coordinators; K3 verifies and removes any residue before closing. None survives as a second generic coordinator. The coalescer inventory is the enumeration in `charters/K3.md`, not a recalled list.

## G3 — Bounded CPU execution and owner-affine commands

**Predecessors:** `B2`, `G2`.

Keep hits/tiny dependent work inline; fork/chunk only coarse owned work; schedule compact commands to owner-affine parse/semantic state; bound queues/fan-out/stacks; reserve foreground/interactive capacity; support local/WASM execution without semantic divergence. Admission is priority-aware; background work coalesces latest-wins with explicit supersession, and total work is bounded by active keys/documents rather than edit count. Whole-server handler-idle is not readiness or admission authority. G3 may provide a reusable bounded-admission primitive; it does not own, accept, or cut over `verter_tsgo_api::actor::ClientHandle::request`. That provider-request reservation, cancellation, and absolute deadline belong solely to H2. Mapper-process JSON-RPC admission belongs solely to TCM2; G3 does not implement that path either.

## G4 — Cache/store convergence and bounded retention

**Predecessors:** `E4`, `G1`, `G2`, `G3`.

Classify each current store, preserve correct value-side validation, remove duplicate correctness invalidation/dedupe, index bounded semantic candidates by snapshot-independent `QueryIdentity`, make return-only default, and enforce weight, pressure, pin, and shutdown contracts.

## G5 — Scheduler/pool/host runtime convergence

**Predecessors:** `G3`, `G4`.

Delete semantic DAG duplication, duplicate pools/dedupe/task taxonomies, and transitional generation machinery only after complete replacement. Preserve ordered mutation and external protocol actors in their real owners. The surviving runtime uses bounded queues, reserved foreground capacity, priority-aware admission, latest-wins background coalescing, active-key/document work bounds, and explicit supersession. G5 deletes global-handler-idle scheduling and tsserver interactive-idle scheduling as readiness/admission mechanisms, including their wait/counter call sites and the timing policies `DISCOVERY_IDLE_GRACE`, `BACKGROUND_MAX_DEFER`, and `BACKGROUND_IDLE_GRACE`; background work runs when capacity permits rather than after arbitrary whole-server idleness. K3 verifies no host/LSP bridge retains the removed scheduling policy.

# 11. Track H — Managed incrementality, providers, and publication

## H1 — Immutable stable-unit incremental reuse

**Predecessors:** `B6`, `F1`, `G4`.

Apply ordered edits, reconcile logical units, reuse unchanged stage artifacts, recompute on value-side validation failure, recompose mappings, keep fallback visible, and prove clean final equivalence.

## H2 — Project-scoped ProviderHub bindings

**Predecessors:** `G5`, `H1`.

Implement explicit capability-declared provider routes/epochs, no racing or silent semantic fallback, demand-scoped companions, controlled transitions, and orthogonal native enrichment. Keep `verter_tsc` a narrow batch-checker boundary.

**Readiness acceptance:** providers mint exact applied-generation or “Program ready for basis” receipts. Queue admission is inside cancellation and the one absolute deadline; no new deadline begins after admission. H2 is the sole acceptance and cutover owner of that provider-request operation, including replacing `verter_tsgo_api::actor::ClientHandle::request` admission-before-select, the post-dequeue timeout in `Actor::serve_one`, and the 2 ms `wait_cancelled` atomic poll with exact event-driven cancellation and one admission-through-response deadline. G3 may supply a reusable bounded-admission primitive that H2 consumes; G3 does not implement this call. Foreground demand awaits or promotes the exact prerequisite. H2 owns serialized `ProviderHub` lifecycle and cross-basis protocol coalescing (the entire IDE-repair mixed cutover of `ide_sync_repair_locks` / `IdeSyncRepairLease` / `IdeSyncRepairLane` open/close/reopen/repair and generation ABA, `ResyncCoordinator` pending-rerun folding, `TsserverCarrierRefresh` latest-generation runner, shared-tsgo `CarrierSyncState`/`PendingSubmission` latest-pending coalescing, `DeclOverlayOwner` overlay serialization with preserved `root_reconcile_epochs` stale-pass high-water, `LazyTransport`/`LazyOverlayCore` establishment singleflight, `ProjectTsserverProvider` keyed `OnceCell` spawn, `LazyManagedTypeProvider` activation, membership-recovery cooldown); G2 supplies only exact-basis production waiters those operations join. Mapper-process JSON-RPC admission is TCM2, not H2. H2 deletes the completion backoff sequence in `server/nav_features.rs` (`[50, 150, 300]` milliseconds), converts `provider_query_with_bounded_recovery` and its receipt-less `Future<Output = ()>` resync callers to exact applied-receipt authorization, and removes `recover_companion_membership` plus the `recovery_attempts < 2`/`yield_now` hover and diagnostics retry loops. At most one retry may follow an exact provider-applied receipt where the protocol genuinely requires reopening; transport completion is never inferred from sleep or pseudo-idle. The shared-overlay 20-second fallback bound is an unsettled serving-policy duration; H2 does not reclassify, retarget, shorten, or delete it without a separate architecture ruling and exact timer tests. Real-process shared-provider tests are serialized, resource-isolated, protocol-ready, and bounded by one independent real watchdog. G2 owns and deletes reusable exact-basis flight mechanics, H2 owns and deletes the displaced lifecycle/coalescing and retry/admission mechanisms, G5 deletes pseudo-idle scheduling, and K3 verifies residual closure.

**Readiness-protocol cutover:** `VerterReady`/`VerterReadyParams` (`$/verter/ready`) and `TypeProviderSyncComplete`/`TypeProviderSyncCompleteParams` (`$/verter/typeProviderSyncComplete`) are transitional global signals, not final readiness. Their `gen` fields currently mix initialization generations from `background_init` and `sync_orchestration::notify_editor_carrier_store_changed` with per-document content-transition generations from `SyncCoordinator`; those incompatible meanings cannot authorize a query. H2 supplies the exact provider-applied basis, H3 cuts every producer and consumer over to per-demand settlement, and K3 deletes the old protocol/latch/wiring ladder after that cutover.

## H3 — Atomic readiness and stale-safe publication

**Predecessors:** `F1`, `H1`, `H2`.

Publish generated companion and required `SourceProjectionMap` atomically; wait only for requested facts; validate observed document/project/config/provider/mapping/dependency stamps; bound channels and protect interactive capacity.

**Settlement acceptance:** foreground hover, completion, navigation, references, rename, code actions, semantic tokens, inlay hints, and similar user requests never wait for edit debounce; they join or promote the exact producer flight immediately. Background diagnostics/indexing may use a quiet window with latest-wins coalescing. Each background quiet-window domain has one lifecycle owner; LSP debounce and edit-triggered import publication share one quiet-window policy and one lifecycle owner (`SyncCoordinator`); no detached sleeping task is spawned per edit. Cross-file edits causally invalidate exact dependents. Replies/publications carry their basis and are discarded when superseded. A fast stale response is a correctness failure, never a latency success. Unrelated edits do not invalidate unrelated requests.

H3 refines the current `SyncCoordinator` as the one lifecycle owner of the LSP quiet-window domain. LSP debounce and edit-triggered import publication share that one quiet-window policy and this one owner; H3 converges the duplicate import-publication edit quiet window into `SyncCoordinator` and does not retain a second owner for that duration. It deletes detached import-publication and `DocumentRegistry::{schedule_semantic_analysis,spawn_semantic_analysis}` per-edit sleeping paths, `semantic_serial` scheduling, `last_change_ms`/`is_typing_cooldown` foreground gates, capture-without-join publication behavior, displaced IDE-repair call paths, and public `LspConfig` topology switches `suppress_edit_debounced_import_publication` and `suppress_sync_coordinator_signal`. The current unjoined `HoverProvenanceCache` producer either migrates through G2 on an F1/F2 exact document-plus-dependency basis with current-basis validation immediately before insert and use, or is deleted; stale behavior is not retained as compatibility.

H3 also cuts all writers, latches, and consumers of `$/verter/ready` and `$/verter/typeProviderSyncComplete` over to exact per-demand receipts, including `background_init` scanner/announcement latches, `sync_orchestration`, `SyncCoordinator`, `packages/language-shared`, `packages/vue-vscode`, `packages/dx-harness`, and benchmark/E2E wiring. K3 is the actual deletion owner for that obsolete global protocol ladder after consumer cutover and the residual-verification owner for all H3 removals.

# 12. Track J — CSS least-work convergence

## J1 — Reconcile CSS syntax, formatter, scanner, and transform owners

**Predecessors:** `A4`, `A6`.

Preserve `StyleSyntaxIr` where it owns syntax; inventory specialized fast paths, preprocessors, CodeTransform mappings, and all consumers. A specialized path survives only when it shares authority and wins equivalent-work proof.

## J2 — Exact style identity and owner-domain reuse

**Predecessors:** `B1`, `B2`, `J1`.

One live parse per exact bytes/dialect/options/domain within its owner; changed bytes create a new identity; pressure reparse is explicit.

## J3 — Shared plans and terminal materialization

**Predecessors:** `B6`, `J2`.

Fuse walks/edit plans where semantics permit; avoid copies for unchanged output; build runtime maps/provenance/descriptors/serialization only when requested.

## J4 — Dialect, preprocessor, formatter, and recovery contract

**Predecessors:** `J3`.

Declare Native/External/Unsupported per operation; make processed bytes/maps/dependencies/fingerprints explicit; prove deterministic idempotent recovery-aware formatting and no private duplicate grammar.

# 13. Track K — Framework catalog, typed carriers, and host decomposition

## K1 — Capability-composed framework catalog

**Predecessors:** `C4`, `E3`, `H3`, `J4`.

Provide immutable coarse dispatch, typed Vue/Svelte capabilities, a synthetic alternate-shape fixture, and monomorphic inner loops. No universal framework IR/fact/cache/options owner.

## K2 — Typed framework options and carriers

**Predecessors:** `B6`, `K1`.

Keep shared axes truly shared; make framework-private requests typed; remove final `Any + Send + Sync` downcasts; keep direct carriers free of managed erasure/thread-safety costs.

## K3 — Reduce/retire `VerterHost` and catch-all session ownership

**Predecessors:** `E4`, `G5`, `H3`, `K2`.

Extract each invariant only after its final owner exists, migrate all consumers, delete sideways mutable access and dependency cycles, and retain only a small stable facade where product compatibility requires it.

**Deletion acceptance:** after G2/G3/G5/H2/H3 install their final owners and perform their assigned deletions, K3 verifies every named transitional host/LSP readiness bridge in `charters/K3.md` is absent and deletes any missed residue before closing. K3 directly deletes the old global `$/verter/ready` / `$/verter/typeProviderSyncComplete` notification, latch, writer, and consumer ladder after H2/H3 cut over to exact per-basis settlement. This residual inventory includes the IDE-repair mixed map/lease/lane lifecycle (H2; G2 supplies only exact-basis production waiters a repair joins), `external_ts_sync` query-dedupe flight and `CarrierPublicationStore` publication lanes (G2), import-publication flights and duplicate quiet-window paths, provider-resync / tsserver carrier-refresh / shared-tsgo `CarrierSyncState` / `DeclOverlayOwner` serialization / `LazyTransport` establishment / keyed tsserver `OnceCell` spawn / lazy-managed activation / membership-recovery coalescers (H2), public suppress-topology `LspConfig` switches (H3), global handler-idle and tsserver interactive-idle scheduling, receipt-less provider retry, and the tsgo admission/deadline/cancellation path (H2 solely). Binding ruling fences in `charters/K3.md` — including the shared-overlay 20-second fallback — are not residue. K3 close dispositions every NAMED row in `charters/K3.md`; it does not re-run a search and an unnamed same-key coalescer is not a close failure (`rulings/MAINTAINER-RULING-COALESCER-CLOSURE-IS-NAMED-DISPOSITION.md`). Dropping a K3 deletion row while G2/H2 still names the cell is a close failure. No unnamed “temporary” coordinator survives.

# 13a. TypeScript content-mapper train (`TCM0`–`TCM4`)

These blocks already exist in `program-dag.toml`. This section does not add or remove a DAG block or a ledger row, does not change TCM status, and does not reopen TCM0's recorded open gaps. It binds the four-part timing taxonomy to the surfaces those charters already own.

## TCM0 — Current TypeScript contract and dual-plane architecture lock

**Predecessors:** `A6`. Read-only lock. Owns no live queue, flight, coalescer, or protocol-admission surface. No extra timing-taxonomy acceptance criteria.

## TCM1 — Compact mapping products inside `CodeTransform`

**Predecessor:** `TCM0`. Mapping-product geometry under CodeTransform-is-sole-authority. Owns no coalescer. The 300 ms background-diagnostics quiet window remains H3's `SyncCoordinator` policy; TCM1 must not widen it. No extra concurrency criteria.

## TCM2 — Content-mapper projection plane

**Predecessors:** `TCM0`, `TCM1`. Dormant until TCM4.

**Timing acceptance:** the `@verter/typescript-content-mapper` JSON-RPC/stdio boundary is external liveness — protocol completion plus one independent real monotonic watchdog; never polling and never inferred from sleep or pseudo-idle. Bounded queue admission (message size, queue depth, outstanding work, handles, caches) sits inside cancellation and one absolute deadline. TCM2 is the sole owner of that mapper-protocol admission; G3 may supply a reusable bounded-admission primitive and must not implement, accept, or delete the path. TCM2 does not introduce a generic coordinator duplicating G2's `FlightCell`.

## TCM3 — TypeScript semantic capability closure

**Predecessors:** `TCM0`, `TCM1`. Dormant until TCM4.

**Timing acceptance:** snapshot acquire/query/release is owned causal progress (generation-validated, snapshot-bound). Bounded concurrent oracle queries consume G2 `FlightCell`; TCM3 owns snapshot scope, cancellation-by-fresh-snapshot, and stale-generation rejection, and does not ship a second generic flight system. The TypeProvider closure remains TCM3/TCM4.

## TCM4 — Atomic activation and deletion

**Predecessors:** `TCM0`, `TCM1`, `TCM2`, `TCM3`.

**Timing acceptance:** activation and deletion land as one owned causal-progress cutover; no dual-path intermediate. The Project-Bound External-TS CRITICAL rule remains in force. Performance obligations are performance measurement and are not waived because the API is new. Creating an unnamed same-key coalescer on the activated path remains FORBIDDEN as a design rule, but proving its absence by search is not required and TCM4 close does not re-run the K3 enumeration (`rulings/MAINTAINER-RULING-COALESCER-CLOSURE-IS-NAMED-DISPOSITION.md`: closure is disposition of the named inventory).

# 14. Track L — Soak, performance, tuning, and final lock

## L1 — Long-churn bounded-memory soak

**Predecessors:** `E4`, `G4`, `H3`, `J4`, `K3`.

Run the `A6`-locked edit/create/delete/rename/move/open/close, project/config/provider restart, query-storm, pressure, cancellation, idle, and quiescence campaigns. Prove clean equivalence, attributable pins, owner plateaus, no monotonic count growth, and that no process restart is required for cleanup.

**Timing/convergence proof:** prove no stale publication after a newer revision; bounded work during rapid edits; foreground bypass of background quiet windows; independent waiter cancellation; exact cross-file convergence; and no correctness dependency on host scheduling. Correctness evidence uses causal receipts/state/work counts, not elapsed thresholds or retry-until-time-passes loops.

## L2 — Final equivalent-work performance decision

**Predecessors:** `B6`, `C4`, `D8`, `E4`, `H3`, `J4`, `K3`, `L1`.

Run every locked absolute SLO, self no-regression, work/copy/allocation, boundary, and competitor/Pareto cell. The primary direct suite must meet its locked best-in-class target. A miss is blocking; it cannot be waived by a post-result ADR. If the product/equivalent-work premise was materially wrong, amend the architecture and Implementation Lock Record under the blind recalibration rule, invalidate the affected candidate evidence, and restart the cell/block.

**Timing benchmark contract:** report distributions rather than one hard timing threshold and distinguish first response, first non-empty response, first correct response, convergence, work started, work superseded, work published, and work discarded. Every latency distribution is paired with exact content/correctness predicates and work attribution, so a fast stale response cannot count as success. Gate-performance changes remain independently measured and are not credited to runtime readiness work without controlled evidence.

## L3 — Optional post-architecture micro-optimization

**Predecessors:** `L2`.  
**Optional:** open only when profiles show a remaining strategic bottleneck.

Allocator, arena pooling, LTO/PGO, SIMD, hashing/container, or executor specialization may proceed only without reopening authority/lifetime boundaries. If opened, it must be accepted before `L4`.

## L4 — Final architecture lock

**Predecessors:** `L2`; and `L3` if opened.

Make code, architecture, ADRs, capability truth, protocols, and tests agree; remove obsolete plans/charters/shims/guards/campaign comments; pass final exact-SHA conformance, architecture, adversarial performance/memory, and maintainer acceptance.

**Exit:** one simpler, deterministic, bounded, measurably leading or non-dominated production architecture.

# 15. Abort and rescope conditions

Stop and return to scope when:

- a hidden owner/consumer/protocol lies in the real cutover closure;
- a required predecessor was neither accepted nor a valid lower layer in the same immutable stack snapshot, or acceptance was attempted before the predecessor landed;
- a profile field or identity dimension is ambiguous;
- a compile plan/facts/load attempt cannot be replay-safe;
- an unknown flow obligation could be ignored as complete;
- a second selectable semantic/parser/resolver/cache-correctness path would survive;
- public/wire compatibility prevents the promised deletion;
- a performance gate would need to be weakened after candidate direction is known;
- correctness requires stale/partial output or unbounded retention;
- native and WASM/local semantics would diverge;
- tests do not discriminate the claimed invariant.

The remedy is an amended contract/ADR/charter and renewed review—not a hidden flag, shim, broad trait, duplicate cache, parallel implementation, or outcome-driven threshold.
