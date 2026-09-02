# Rev11 flow train authority correction (D1–D8)

- Status: accepted
- Date: 2026-08-29
- Supersedes: the stale owner/API/deletion language mechanically repeated across the rev11.flow D-train charters
- Scope: rev11.flow charters D1–D8 and their `authority/dag/rev11-flow.toml` mirror; no production code

## Context

Dispatch of D1 hit its own abort condition: current source disproves the named owner/API boundary. The D-train charters named **legacy flow resolver paths** as the current owner and `FlowSlice` / `ResultContract` as API/data boundaries. Source shows the opposite. The sole flow authority already exists — `ProjectSemanticDispatch` / `execute_function_return_source` (`crates/verter_session/src/project_semantic_dispatch/flow_return.rs`) over the flow substrate in `crates/verter_semantic/src/analysis/flow`, with `crates/verter_session/src/flow_slice_content.rs` as its content-lowering half — and no parallel flow resolver exists anywhere. `FlowSlice` and `ResultContract` do not exist; the real types are `FlowSliceIR` and `ResultContractId` (`crates/verter_identity/src/identity.rs`), the latter not yet integrated into flow-return production.

Evidence, all confirmed by the maintainer:

- A5 owner rows (`docs/arch/refactor/rev11/evidence/A5/owner-rows.md` at `0e121f964`): `flow_slice_content.rs` is part of the existing flow substrate, not a parallel resolver, and the row had to be restated before D2 received a charter.
- A6 implementation lock record and capability matrix (`docs/arch/refactor/rev11/evidence/A6/implementation-lock-record.md`, `docs/arch/refactor/rev11/contracts/capability-matrix.md` at `0e121f964`): a charter requiring deletion of a second engine would target something that does not exist.
- AMD-004 (`docs/arch/refactor/rev11/amendments/AMD-004-defer-completion-to-d6.md` at `0e121f964`): exact structural completion and G10 discrimination are deliberately deferred to D6/U6 behind a closed code-first carrier inventory.
- The A3 wrong-complete retraction is landed (`authority/state/implemented.toml`); the residual non-call fabricated-`any` fallback is recorded debt RESIDUAL-NON-CALL-ANY-FABRICATION, durably owned by U6.VALUE_INFERENCE (`crates/verter_session/tests/cases/manifest_data/typeinfo_guard_registry.rs:599`).

These historical rev11 documents were removed from the live tree by the Tama transition and remain evidence, not live authority.

## Decision

Maintainer ruling, applied as an authority correction before any production code:

1. D1 is paused and the D train is amended before production code. D1 is not marked complete, not skipped, and not jumped over.
2. D1 is re-chartered as the private missing foundation inside the preserved sole dispatch and flow substrate: closed domain/operation registry, deterministic demand plan, stable binding slots, obligation ledger, typed gaps, exact parse reacquisition, and a proof-bearing private finalizer.
3. The A6 capability matrix must be ratified before D2's public cutover. This may proceed alongside D1 and is recorded as external requirement `maintainer_a6_capability_matrix_ratification` on the D2 charter header and DAG node. *[Superseded: the same-day maintainer waiver recorded in `2026-08-29-rev11-flow-d1-hermetic-checkpoint.md` §6 WAIVES this gate — the external requirement was removed from the D2 charter header, DAG node, predecessor-contract bullet, and abort conditions; D2's cutover no longer waits on matrix revision or ratification.]*
4. D2 is re-chartered as the atomic product cutover. Only source-verified displaced responsibilities are deleted — absence of proof means preserve — and `flow_slice_content.rs` is a preserve/converge/absorb decision made against source, not a predeclared second engine.
5. D3 → D4/D5/D6 → D7 → D8 continue. D6 owns the AMD-004 deferred residual completion behind the closed carrier inventory, with AMD-004's architectural constraints binding (content-free skeleton topology; demanded `FunctionFlowGraph` sole completion reducer; no second classifier; no syntax-only G10 fallback; A3 responds only to typed `FlowGap` information). RESIDUAL-NON-CALL-ANY-FABRICATION remains owned by U6.VALUE_INFERENCE and gates the U6 lane close at D8.
6. D1 and D2 land as one explicit atomic multi-node candidate with an internal D1 checkpoint; D1 was never intended to merge independently. D2A is the intentional unmapped exception: D1, D2A, and D2B keep distinct ledger rows, while D1 and D2B retain their issue mappings and closing links per `contracts/github-control-plane.md`.

Charter corrections applied across D1–D8: fictitious `FlowSlice` / `ResultContract` boundaries replaced with source-verified names (`FlowSliceIR`, `ReturnSlicePlan`, `FlowSliceHash`, `SemanticQueryKey`, `FlowGap`, `FunctionFlowGraph`, `ResultContractId` as scope-appropriate); “current owner is legacy flow resolver paths” replaced with the actual sole dispatch; “delete parallel flow resolver / wrong-complete fallback” replaced with the no-second-engine law and the AMD-004 debt ownership; D6's stale `diagnostic_action_service` conflict domain corrected to `flowslice` in the charter header and DAG node.

## Consequences

- D1's abort condition is resolved: every charter now names owners and boundaries that exist in source.
- No charter may claim to delete the residual non-call fabricated-`any` fallback; its ledger debt is owned by U6.VALUE_INFERENCE and the U6 lane may not close with it open.
- `dispatchable`, budgets, review profiles, and the DAG topology are unchanged; the correction is authority text plus the D2 external-requirement field and the D6 conflict-domain field.
- The D1/D2 node names are unchanged to avoid DAG churn; their H1 titles and bodies carry the corrected framing.
