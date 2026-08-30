# Rev11 flow D3 split into the D3R/D3I/D3P/D3C atomic candidate (codex scope ruling)

- Status: accepted
- Date: 2026-08-30
- Supersedes: nothing; narrows the D3 node created by `decisions/2026-08-29-rev11-flow-authority-correction.md` into four nodes, extending the split pattern of `decisions/2026-08-29-rev11-flow-d2-split.md`
- Scope: rev11.flow DAG node D3 and its charter, ledger issue mapping, and downstream predecessor references; no production code

## Context

An orchestrator-accepted codex architecture ruling (read-only, against committed `HEAD 8db253e6b`) held that D3 is the semantic product-kernel cutover — not another registry, obligation, or admission-proof patch — and that a single D3 cannot honestly fit its 800 production LOC / 8 production files budget: the minimum sound work combines the relation cutover (live bounded `Identity` and tri-state `Comparable`, deletion of the flow-private `NodeDisjointness`/`nodes_provably_disjoint` classifier), the completion of stable binding identities (deduplication removal, real destructured slots, `FlowBindingRef`/exact `FlowBindingMap`), the product lattice substrate, and the evaluator/worklist cutover — at least thirteen production files and two independent correctness concerns, against a source carrying over 300 name/state references in the 9,139-line `flow_return.rs`. The ruling therefore requires amending D3 into an atomic four-node candidate before any implementation mutation.

## Decision

Authority text only, per the ruling:

1. **D3 is replaced by four nodes**, all train `rev11.flow`, kind `implementation`, semantic role `delivery`, resource class `rust-mixed`, gate profile `targeted-domain`, efforts high, size M, and budgets 800/8/2 with 1500/12/3 rescope:
   - `D3R — Nominal relation authority` (predecessors D2B, C1; class `foundational`; conflict domain `flowslice`; review profile `architecture-3`): live bounded `RelationKind::Identity` and tri-state `Comparable` (holds / does-not-hold / undecided via the existing `RelationStep`/ReturnOnly rails, no public `Unknown` payload), reusing `ValueDeclIdentityPart` and the existing unique-symbol lookup with identity preserved through aliases/imports/re-exports; deletion of `NodeDisjointness`/`nodes_provably_disjoint`; `Subtype`/`StrictSubtype` stay pending; `call_resolve` stays on `Assignable`; `Relate` remains the sole query tag.
   - `D3I — Complete stable binding identity` (predecessor D3R; same class/profile/domains): removal of the `function_program.rs` binding deduplication, `FunctionBindingKind` extended across all value-bearing `SkeletonBindingKind`s, every destructured bound identifier indexed with a real slot, `FlowBindingRef::{Local, Captured}`, an exact name-fallback-free `FlowBindingMap`, and conversion of `SliceStatement::Binding` / `SliceExpr::Local` / `SliceNarrowRoot` / capture authorities to resolved binding references.
   - `D3P — Product lattice substrate` (predecessors D3R, D3I; same class/profile/domains): new `project_semantic_dispatch/flow_products.rs` with `FlowProductKey`/`FlowProductValue`/`FlowProductStore`/`ReachingValueProduct`/`DefiniteAssignment`/`FlowTransferOutcome`, exhaustive `transfer_product`/`join_product` per live domain, and extension of the closed `FlowDomain` registry with `DeclaredType` and `DefiniteAssignment` (intentional `ResultContractId` contract-version change exercising D2B's exact-contract tests). Production-compiled but without any cutover.
   - `D3C — Product worklist cutover` (predecessor D3P; class `foundational-atomic`; conflict domains `flowslice`/`semantic_authority`; review profile `public-3`): the indivisible cutover — `FlowEvaluator` state replaced by `FlowProductStore`, `join_layer_states` replaced by domain joins, selected transfers executed in `FlowDemandPlan` order under the connected `max_iterations`/selected-frontier budget, product discharge evidence through the existing `FlowDischargeReport`/finalizer, deletion of all `String`-keyed runtime semantic state, rehoming of the narrowing state without enlarging D4 semantics, and literal-widening provenance riding the reaching-type product.
2. **Atomic landing preserved and extended.** D3R, D3I, D3P, and D3C land as ONE atomic multi-node candidate; none of the four merges independently. This extends the D1+D2A+D2B atomic-landing ruling (`decisions/2026-08-29-rev11-flow-d2-split.md`) without weakening it — none of the intermediate nodes is releasable independently.
3. **Issue mapping rekeyed.** The `[[github_issue]]` ledger row for D3 (gh_issue 175, `sync_to_github = true`) is rekeyed to `D3C` — the cutover node keeps the issue. D3R, D3I, and D3P intentionally carry NO GitHub issue mapping: the maintainer has frozen GitHub issue churn, and no new issues are created for the substrate nodes.
4. **Downstream references repointed.** D4, D5, and D6 predecessors become D3C in `authority/dag/rev11-flow.toml` and in their charters' predecessor fields and "Exact predecessor contracts" bullets. `charters/rev11-flow/D3.md` is deleted; `D3R.md`, `D3I.md`, `D3P.md`, and `D3C.md` are authored with the ruling's per-node scopes and the thirteen required discriminating tests distributed across the four charters.

## Consequences

- D3R is READY on the trusted ledger alone (D2B and C1 are implemented); the chain then gates D3I on D3R, D3P on D3R+D3I, and D3C on D3P.
- D3C carries the cutover's full review weight (`public-3`) and the rekeyed gh_issue 175 mapping; its charter names the product-evidence discharge test and the successor-boundary controls.
- The DAG topology grows by three nodes; `dispatchable`, budgets, and review profiles elsewhere are unchanged. D4–D6 depend on D3C; D7/D8 topology is untouched.
- Earlier decision records' D3 references remain historical context; this record is the binding authority for the split.
