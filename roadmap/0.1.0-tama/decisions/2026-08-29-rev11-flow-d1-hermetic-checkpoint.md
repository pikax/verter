# Rev11 flow D1 hermetic checkpoint and cutover-gate correction (authority round 2)

- Status: accepted
- Date: 2026-08-29
- Supersedes: the round-1 D1/D2 cutover framing and RESIDUAL-NON-CALL-ANY-FABRICATION debt-ownership phrasing in `decisions/2026-08-29-rev11-flow-authority-correction.md`, where they conflict
- Scope: rev11.flow charters D1–D8 and their `authority/dag/rev11-flow.toml` mirror; no production code

## Context

A maintainer-directed review of the round-1 authority correction found semantic blockers:

1. **D1/D2 cutover distinction erased.** Round-1 D1/D2 read as one continuous strengthening of the existing sole dispatch. The correct model (flow-completeness contract §6, `docs/arch/refactor/rev11/contracts/flow-completeness.md` at `0e121f964`): D1 builds a PRIVATE, HERMETIC cutover candidate — unreachable from production entry points — sharing the ONE structural graph authority (`FunctionProgramIndex` → `FunctionBodySkeleton` → `FunctionFlowGraph`, contract §1) and the shared relation/inference authority; D2 atomically swaps production execution onto it. D1 is a review checkpoint, never independently mergeable.
2. **Concepts mapped onto wrong current types.** `FlowSlotId` / `FlowExprId` (`crates/verter_semantic/src/analysis/flow/flow_ir.rs:77`, `:93`) are dense slice-local indexes, not stable binding authority — the stable slots are `SkeletonBindingId` (`flow_ir.rs:19` import, `FlowSlot.binding` at `:148`) plus the cross-frame `FlowBindingIdentity` (`crates/verter_semantic/src/analysis/function_program.rs:275`). `ReturnSlicePlan` (`flow_ir.rs:32`) is graph reachability selection, not the contract §3 `FlowDemandPlan` (domain/result-contract/obligation plan). `FlowEffect` (`flow_ir.rs:284`) records evaluation effects, not solve-obligation states or discharge evidence. `FlowReturnResult` (`crates/verter_session/src/semantic_query/flow_return_result.rs:50`) still has three distributed admission channels (its module doc), so it is NOT the proof-bearing completeness finalizer — D1's finalizer is the single private construction point per contract §5, and the distributed channels are the legacy admission path D2 retires. And the obligation ledger must EXTEND the existing `ObligationRuntime` (`crates/verter_session/src/project_semantic_dispatch/dispatch_txn.rs:879`), never introduce a peer ledger.
3. **D2's A6 gate underspecified.** The current A6 capability matrix cannot enumerate effective-flow capabilities. The gate is: explicit effective-flow rows or an explicit row-to-flow-subcapability mapping added to the matrix FIRST, THEN maintainer ratification — not mere ratification of the current matrix (D1-SIX-FORKS Fork 4: a capability-matrix revision, explicitly not a program amendment).
4. **Debt ownership misassigned.** AMD-004 assigns exact structural completion / G10 discrimination to D6. RESIDUAL-NON-CALL-ANY-FABRICATION is a SEPARATE debt (debt record, `crates/verter_session/tests/cases/manifest_data/typeinfo_guard_registry.rs:599`) owned by U6.VALUE_INFERENCE (the shallow pass's per-expression fallback: JSX / Super / MetaProperty / ImportExpression / ClassExpression) plus its named async/call owners U6.ASYNC_GENERATOR and U6.CALL_RESOLVE (`await x`), gating the U6 lane close at D8. Round-1 charters wrongly said D6/D8 own it.

Binding authority, all as historical evidence at `0e121f964` (the Tama transition removed these from the live tree; they are evidence, not live authority):

- Flow-completeness contract §1 (sole structural authority), §2 (closed operation/domain registry), §3 (`FlowDemandPlan`), §4 (obligation ledger, `ObligationState`), §5 (`FlowSolveOutcome`, private `CompleteFlowResult` constructor, finalizer preconditions), §6 (D1 private hermetic candidate; D2 indivisible cutover steps 1–5), §7 (required tests).
- D1-SIX-FORKS verdicts (`docs/arch/refactor/rev11/rulings/ARCH-RULING-D1-SIX-FORKS.md`): Fork 1 — the A6 matrix is an unratified target; D1 may start, D2 may not start before matrix ratification. Fork 3 — same obligation authority: extend `ObligationRuntime`, no peer ledger. Fork 4 — effective-flow rows are not enumerable from the current matrix; add explicit flow rows/mapping. Fork 5 — reuse existing binding-slot identities; do not mint a parallel abstraction.
- C1-D1-FLOW-FILE-RECONCILIATION header verdict (`docs/arch/refactor/rev11/rulings/ARCH-RULING-C1-D1-FLOW-FILE-RECONCILIATION.md`): `flow_slice_content.rs` and `flow_return_callee.rs` MOVE whole to `verter_semantic`; `flow_return.rs` and `dispatch_txn.rs` SPLIT. C1 is landed and current source is truth: all four files still live under `crates/verter_session/src`, so the charters cite the current paths.

## Decision

Maintainer-directed correction round 2, applied as authority text only:

1. D1 is reframed as the private hermetic checkpoint: it builds the contract §2–§5 foundation (closed domain/operation registry, `FlowDemandPlan`, `SkeletonBindingId`/`FlowBindingIdentity` binding slots, the extended `ObligationRuntime` with typed `ObligationState`, typed `FlowGap`, and the `FlowSolveOutcome`/`CompleteFlowResult` private-constructor finalizer) behind a private hermetic test boundary, unreachable from product entry points, landing only inside the atomic D1+D2 candidate. The round-1 type mappings (`SliceDemand`→`ReturnSlicePlan`, `FlowSlotId`/`FlowExprId` slots) are retracted. D1-AC1..4 are mapped onto the contract §7 required tests.
2. D2 is reframed as the indivisible contract §6 cutover: route every public effective-flow operation to the new solver; delete the old evaluator and its state, caches, tasks, flags, compatibility shims, and migration guards — each deletion citing a source-verified displaced route (absence of proof means preserve); typed gaps for unimplemented mechanisms; preserve every ratified Supported/Stable capability and failure contract; prove no second selectable evaluator remains. The three distributed `FlowReturnResult` admission channels are part of the retired legacy admission path.
3. The D2 external requirement is renamed `maintainer_a6_flow_capability_rows_ratification` (charter header and DAG node) and reworded: explicit effective-flow rows or a row-to-flow-subcapability mapping added to the A6 matrix FIRST, then maintainer ratification. D1 may start before this gate is met; D2's cutover may not land before it.
4. Debt ownership corrected in every charter: D6 owns ONLY the AMD-004 debt (exact structural completion + G10 discrimination behind the closed code-first carrier inventory, AMD-004 constraints binding). RESIDUAL-NON-CALL-ANY-FABRICATION is owned by U6.VALUE_INFERENCE plus U6.ASYNC_GENERATOR and U6.CALL_RESOLVE; D8 verifies closure and may not close the U6 lane while the entry is open, but owns no part of the debt.

5. Codex architect review of the round-2 diff returned TRAIN HOLD on two residual contradictions, both fixed as prescribed: D1's no-second-engine law is now scoped to **production reachability** with the single private hermetic candidate as the one permitted construction (contract §6); D2's named API/data boundaries now include `FlowReturnResult`, so retiring its three distributed admission channels is inside D2's declared mutation scope.

6. Maintainer waiver (2026-08-29): the A6 capability-matrix gate is WAIVED for D2. The external requirement `maintainer_a6_flow_capability_rows_ratification` is removed from the D2 charter header, the D2 DAG node, the predecessor-contract bullet, and the abort conditions; D2's cutover no longer waits on matrix revision or ratification. The capability-matrix revision remains good practice but is not a D2 gate.

The maintainer explicitly refused GitHub issue prose updates: issues #173/#174 remain as-is, and no `catalogs/github-issue-content.toml` entries are authored. This record and the charters carry the correction instead.

Resolved by maintainer direction (2026-08-29): no pull requests. D1+D2 land directly on the shared `train/rev11-flow` branch and are pushed to origin; each node keeps its own issue mapping, ledger row, and one `Closes #<gh_issue>` line in the landing commit body per the non-PR closing flow in `contracts/github-control-plane.md`. Issues close when the train branch reaches the origin default branch; Project 3 `done` is marked only after that point. No `githubctl create-pr`/`squash-land` extension is built.

## Consequences

- D1 can dispatch as a private hermetic checkpoint with source-verified type boundaries; its abort condition no longer fires on retracted concept mappings.
- D2's cutover gate is now enforceable as a two-step external requirement (rows/mapping, then ratification), and its deletion discipline names the legacy admission path.
- RESIDUAL-NON-CALL-ANY-FABRICATION ownership is consistent across D1–D8 and matches the debt record; no charter assigns it to D6 or D8.
- `dispatchable`, budgets, review profiles, and the DAG topology are unchanged; the correction is authority text plus the renamed D2 external-requirement field.
