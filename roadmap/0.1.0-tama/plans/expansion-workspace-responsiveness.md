# Plan: expansion.workspace-responsiveness

Owner: `expansion.workspace-responsiveness` — large-workspace responsiveness and Lapce hang elimination. Constitution: `contracts/workspace-responsiveness-v1.md` (ratified by WSP0). Product: `wsp_product`.

This plan is the train map for the workspace-responsiveness nodes. Each node's binding content is its charter under `charters/expansion-workspace-responsiveness/`; this file records order, boundaries and status only. Implementation state is resolved solely by the implementation ledger (`authority/state/implemented.toml`), never by this document.

## Node map

| Node | Outcome | Predecessors | Status |
| --- | --- | --- | --- |
| WSP0 | Issue 93 investigation and interactive SLO constitution | ORC0 + MEM0 | implemented (this delivery) |
| WSP1 | Instrumented server and protocol replay harness | WSP0 | pending |

Cross-train successors PER0 (expansion.kernel) and WPF0 (expansion.web-performance) name WSP0 in their own predecessor sets; they consume `contracts/workspace-responsiveness-v1.md` through their own charters and do not import this train.

## Boundaries

- **WSP0 → WSP1.** WSP0 ratifies the workload population, measurement definitions, reference-machine manifest requirements, issue-93 evidence plan and interactive SLO catalog as contracts with planted-negative validation only. WSP1 owns every new performance instrument, the reference-machine recordings and the real project pins; nothing in WSP0 pre-certifies WSP1 code.
- **WSP1 is the bound downstream runtime-test owner** for every SLO and measurement row ratified here (WSP0-AC-BASIS): a docs-only node adds no meaningless runtime test, and no budget is promotable from `pinned-candidate-target` to measured without a WSP1 run against a recorded reference machine.
- **Not semantic execution.** Semantic algorithms, project resolution, format/lint logic, TypeScript projection and edit transactions stay with their producing owners; this train measures and budgets them, it never forks them.

## Rules inherited by every node in this train

- UI responsiveness is measured client-side from input to applied paint and is never certified by server response time alone (WSP0-AC1).
- No reduced feature set, hidden error truncation or reduced project membership counts as a performance improvement; all budgets carry equal-work denominators with all required features enabled (WSP0-AC2).
- Issue 93 stays labelled unverified until reproduced under the ratified workloads; raw failed measurements are retained (WSP0-AC3).
- Total process-tree RSS and language-service incremental RSS are reported separately; every not-yet-instrumented metric is labelled, never guessed as zero.
- No new semantic/type/project engine inside a client, no second status store, no new orchestration system; conflict domains stay `lsp_publication`, `performance_evidence`, `scheduler_admission`.
