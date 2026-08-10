# Verter Architecture Convergence Program — Revision 11

**Status:** Normative dependency-ordered implementation authority.  
**Durable authority:** `architecture.md`, contracts, and accepted ADRs.  
**Proof authority:** `verification.md`.  
**Governance authority:** `governance.md`.  
**Machine-readable predecessor authority:** `program-dag.toml`.

# 1. Program law

The program is a DAG of bounded clean cutovers. A block may start when every predecessor has its required accepted state and the validated `program-state.toml` marks it `READY`/`BLOCK_READY`. A dependent upper layer may instead perform contingent `READY`/`IN_PROGRESS`/`REVIEW` work only when each unaccepted predecessor is a lower layer in the same validated immutable stack snapshot. It cannot become `ACCEPTANCE_RECOMMENDED` or `ACCEPTED` until those predecessors are formally satisfied and the upper candidate is restacked/revalidated on the accepted base. Except for the explicit `D1` private checkpoint, accepted block state means the predecessor is integrated on the intended lineage. `D1` is reviewed on the bounded `D2` integration branch and is never merged/released independently; `D2` is the atomic landing unit. Parallel work is legal only when cutover closures, writable worktrees, and shared generated/owner surfaces are disjoint or explicitly serialized.

A pull-request stack is transport only. Bounded stack windows may linearize one short dependency path for review, but they do not create semantic predecessor authority. A program-wide mega-stack is prohibited. Atomic private layers land only through their final atomic candidate.

Before `A6`, only Gate 0 work is legal. `A3` may change behavior solely to retract a known wrong-complete result to a typed non-admissible outcome. It may not choose a disputed final owner.

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

## A3 — Retract known wrong-complete results

**Predecessors:** `A2`.

Any exposed path that skips an unmodelled obligation returns typed `Partial`, `FlowGap`, or `NoValue` and is not warm-admitted. Authored `any` remains distinct. This is the only pre-`A6` semantic behavior change permitted.

**Exit:** no known wrong-and-warm result masquerades as complete.

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

**Predecessors:** `B3`, `B5`, `C1`.

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

## D6 — Loop fixed points and completion algebra

**Predecessors:** `D3`.

Implement deterministic selected-frontier loop convergence and one completion algebra for labels, switch, loops, try/catch/finally, return, throw, break, and continue.

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

Reconcile current VFS/registered-source/workspace publication into one authority; use short immutable-root commits; keep parse/compile/provider work outside the write critical section; preserve ordered document mutation.

## F2 — InputBasis, load waves, negative facts, and retry

**Predecessors:** `F1`, `C2`.

Implement exact `InputBasisId`, normalized batched `LoadSet`, stable positive/negative observations, conditional coherent commits, no-progress and resource bounds, and clean preloaded equivalence.

# 10. Track G — QueryRuntime, FlightCell, executor, and cache convergence

## G1 — Hermetic query facts and result contracts

**Predecessors:** `C4`, `D8`, `F2`.

Implement snapshot-independent `QueryIdentity`, exact-basis `SemanticFlightKey`, bounded multi-candidate lookup, positive/negative read facts, exact result contract, owner-controlled compute scope, sealed publish/return-only decision, value-side validation, and operation-specific cycle/failure policy.

## G2 — FlightCell-owned same-key production

**Predecessors:** `G1`.

Implement content and semantic flight classes, exact input-basis scoping, independent waiters, policy aggregation, cancellation, panic/shutdown/finalization, follower validation, and no leader-owned lifetime.

## G3 — Bounded CPU execution and owner-affine commands

**Predecessors:** `B2`, `G2`.

Keep hits/tiny dependent work inline; fork/chunk only coarse owned work; schedule compact commands to owner-affine parse/semantic state; bound queues/fan-out/stacks; reserve interactive capacity; support local/WASM execution without semantic divergence.

## G4 — Cache/store convergence and bounded retention

**Predecessors:** `E4`, `G1`, `G2`, `G3`.

Classify each current store, preserve correct value-side validation, remove duplicate correctness invalidation/dedupe, index bounded semantic candidates by snapshot-independent `QueryIdentity`, make return-only default, and enforce weight, pressure, pin, and shutdown contracts.

## G5 — Scheduler/pool/host runtime convergence

**Predecessors:** `G3`, `G4`.

Delete semantic DAG duplication, duplicate pools/dedupe/task taxonomies, and transitional generation machinery only after complete replacement. Preserve ordered mutation and external protocol actors in their real owners.

# 11. Track H — Managed incrementality, providers, and publication

## H1 — Immutable stable-unit incremental reuse

**Predecessors:** `B6`, `F1`, `G4`.

Apply ordered edits, reconcile logical units, reuse unchanged stage artifacts, recompute on value-side validation failure, recompose mappings, keep fallback visible, and prove clean final equivalence.

## H2 — Project-scoped ProviderHub bindings

**Predecessors:** `G5`, `H1`.

Implement explicit capability-declared provider routes/epochs, no racing or silent semantic fallback, demand-scoped companions, controlled transitions, and orthogonal native enrichment. Keep `verter_tsc` a narrow batch-checker boundary.

## H3 — Atomic readiness and stale-safe publication

**Predecessors:** `F1`, `H1`, `H2`.

Publish generated companion and required `SourceProjectionMap` atomically; wait only for requested facts; validate observed document/project/config/provider/mapping/dependency stamps; bound channels and protect interactive capacity.

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

# 14. Track L — Soak, performance, tuning, and final lock

## L1 — Long-churn bounded-memory soak

**Predecessors:** `E4`, `G4`, `H3`, `J4`, `K3`.

Run the `A6`-locked edit/create/delete/rename/move/open/close, project/config/provider restart, query-storm, pressure, cancellation, idle, and quiescence campaigns. Prove clean equivalence, attributable pins, owner plateaus, no monotonic count growth, and no restart cleanup.

## L2 — Final equivalent-work performance decision

**Predecessors:** `B6`, `C4`, `D8`, `E4`, `H3`, `J4`, `K3`, `L1`.

Run every locked absolute SLO, self no-regression, work/copy/allocation, boundary, and competitor/Pareto cell. The primary direct suite must meet its locked best-in-class target. A miss is blocking; it cannot be waived by a post-result ADR. If the product/equivalent-work premise was materially wrong, amend the architecture and Implementation Lock Record under the blind recalibration rule, invalidate the affected candidate evidence, and restart the cell/block.

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
