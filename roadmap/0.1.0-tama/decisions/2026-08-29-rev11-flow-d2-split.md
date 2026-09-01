# Rev11 flow D2 split into D2A substrate and D2B cutover (codex scope ruling)

- Status: accepted
- Date: 2026-08-29
- Supersedes: nothing; narrows the D2 node created by `decisions/2026-08-29-rev11-flow-authority-correction.md` and reframed by `decisions/2026-08-29-rev11-flow-d1-hermetic-checkpoint.md` into two nodes
- Scope: rev11.flow DAG node D2 and its charter, ledger issue mapping, and downstream predecessor references; no production code

## Context

An orchestrator-accepted codex architecture ruling (read-only, issued at `53394e73d`) held that D2 is a completeness/admission cutover, not a replacement value evaluator, and that the indivisible contract-§6 cutover cannot fit D2's 800 production LOC / 8 production files budget: the minimum sound cutover touches at least ten production files, and compressing it into eight would either preserve one of the three distributed `FlowReturnResult` admission channels or leave proof state singleton. The ruling therefore requires splitting D2 into two DAG nodes before any implementation mutation.

The ruling was written against the pre-repair D1. D1 has since been repaired (`62c0fabbd`): the store-minted `BoundFlowGraph` graph identity exists, the complete contract semantics are already hashed into `ResultContractId`, the obligation budget is enforced at insertion, and the finalizer consumes only the sealed `SealedFlowCompletion` artifact. D2A's scope is adjusted accordingly — it builds on the repaired foundation and redoes none of it.

## Decision

Authority text only, per the ruling:

1. **D2 is replaced by two nodes.**
   - `D2A — Canonical flow demand and proof substrate` (predecessor D1, class `foundational-private-checkpoint`, review profile `semantic-3`, conflict domain `flowslice`): production-compiled but publicly unreachable substrate — un-gating `flow_solve` / `flow_obligation_state`, per-demand handles (`Vec<InstalledFlowDemand>` with an unforgeable `FlowDemandHandle` replacing the singleton ledger), the contract-bearing `FlowReturnKey` with `ResultContractId` derived only in `flow_return_key_with_demand`, ONE retained structural plan shared by the hash node, lowering, and the demand plan (`FlowSliceHashOutcome::Planned` carries `Arc<PlannedFlowSlice>`; both duplicate `ReturnPathPeeker` replans are deleted), and the convergence/evidence carriers. No public admission route and no shadow result comparison.
   - `D2B — Atomic public flow-proof cutover and distributed-admission retirement` (predecessor D2A, keeping D2's class `foundational-atomic`, review profile `public-3`, conflict domains `public_protocol`/`semantic_authority`, and budgets): the indivisible contract-§6 cutover — evaluator discharge reports, finalization wiring, root/SCC `CompleteFlowResult` proof tokens, pending typed gaps for `FlowNarrowingAt` / `ContextualTypeAt`, deletion of all three distributed admission channels (each citing its source-verified displaced route), the six discriminating cutover tests, and the acceptance guards. `flow_slice_content.rs` is PRESERVED in place (ruling §4).
2. **Atomic landing preserved and extended.** D1, D2A, and D2B land as ONE atomic multi-node candidate; none of the three merges independently. This extends the maintainer's D1+D2 atomic-landing ruling (2026-08-29) without weakening it — neither preparatory node is separately releasable.
3. **Issue mapping rekeyed.** The `[[github_issue]]` ledger row for D2 (gh_issue 174, `sync_to_github = true`) is rekeyed to `D2B` — the cutover node keeps the issue. D2A intentionally carries NO GitHub issue mapping: the maintainer has frozen GitHub issue churn, and no new issue is created for the substrate node.
4. **Downstream references repointed.** D3's predecessor becomes D2B, as do the E1 (rev11-public-typeinfo) and F1 (rev11-inputs) predecessor references that meant the cutover. `charters/rev11-flow/D2.md` is deleted; `D2A.md` and `D2B.md` are authored; D1's charter prose now names the D1+D2A+D2B atomic candidate and D2B as the public-cutover boundary.

## Consequences

- D2A is READY on the trusted ledger alone (D1 is implemented); it can dispatch as a private substrate checkpoint with truthful 800-LOC / 8-file budgets.
- D2B carries the cutover's full review weight (`public-3`) and the rekeyed gh_issue 174 mapping; its charter enumerates the six discriminating cutover tests and the per-deletion displaced-route citations.
- The DAG topology grows by one node; `dispatchable`, budgets, and review profiles elsewhere are unchanged.
- The round-2 decision record's D2 references remain historical context; this record is the binding authority for the split.
