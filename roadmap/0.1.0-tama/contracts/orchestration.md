# Orchestration contract

## State authority

The static DAG defines nodes and predecessor edges. `authority/state/implemented.toml` defines which nodes are implemented. Every DAG node is predeclared exactly once under `[implementation]`. The transitioned row is the implementation fact.

Each node is a single line:

- `"ID" = { status = "pending" }` — not implemented; pending rows carry no evidence;
- `"ID" = { status = "implemented", commit_message = "...", commit_date = "...", pull_request = N }` — implemented (`pull_request` optional).

Implemented rows carry:

- `commit_message` — a useful, non-exact commit search phrase;
- `commit_date` — an approximate timezone-bearing date used to choose the closest result when a phrase finds several commits;
- `pull_request` — an optional PR number, particularly useful after GH0.

The locator fields are never validated. They do not prove content, authorship, review, ancestry, landing, or identity. Flipping the row back to `status = "pending"` is the deliberate operation that marks a node unimplemented.

## Readiness algorithm

For each node:

1. A row with `status = "implemented"` makes the node COMPLETE.
2. No DAG status, Git state, or historical record makes a node COMPLETE without that implemented status.
3. A dispatchable node becomes READY when every transitive DAG ancestor is implemented.
4. Other nodes are BLOCKED.

Nothing else affects readiness. There is no activation gate, conditional-predecessor state, or declared in-progress status. External requirements, conflict domains, resources, gates, and review profiles are instructions followed by agents and maintainers.

## Candidate workflow

Each independently landable node is one candidate by default: one dedicated worktree and branch, one implementation owner, one squash commit, and—when GitHub control is active—one mapped issue and one pull request. A train manager coordinates ordering but does not accumulate sibling-node mutations in one shared worktree. Multiple nodes may share a worktree, branch, squash, or pull request only when the user or maintainer explicitly requests one atomic multi-node landing before mutation and records why those nodes cannot land independently.

The implementer transitions the node's predeclared ledger line from pending to implemented in its implementation patch before squash or review. The transitioned row names the planned squash title and approximate date. When GitHub control is active, the issue mapping and node worktree/branch exist before mutation, and a draft pull request is opened after the first implementation commit is pushed. The candidate is then squashed once, reviewed according to its profile, and verified according to its gate profile. The reviewed pull request is the landing path and is squash-merged through GitHub; the candidate is not landed locally first and mirrored afterward. For a user- or maintainer-directed non-PR landing, the final squash commit body instead contains one `Closes #<gh_issue>` line for every included node's local mapping before review; normal landing to the origin default branch then performs issue closure.

The candidate's landed source and tests contain no roadmap identity: no program/revision, DAG, node/block/train ID, phase/stage, implementation sequence, deletion history, or DAG-managed issue/PR citation. Comments and test artifacts state durable behavior. A GitHub issue may be cited only for a specific independently reported defect outside the DAG mappings, and only as a supplement to that behavioral explanation.

Production LOC and file budgets are planning references. The implementer and reviewers compare the actual candidate with them and investigate material divergence without treating the numbers as pass/fail thresholds. A large mismatch, such as one expected production file becoming ten, requires a scope-coherence explanation and may reveal work that needs an ordinary DAG amendment.

The lifecycle has no admission, lease, dispatch receipt, candidate-finalization receipt, review manifest, acceptance receipt, landing receipt, activation transition, amendment digest chain, or runtime root. Review reports may remain ordinary task artifacts; they are not machine-bound to Git identities.

## Train conformance and completion review

The train manager maintains a human coordination count of blocks implemented since the train's last architecture checkpoint. After every 3 to 6 newly implemented blocks, it spawns a fresh Codex Architect conformance review of the train's cumulative implementation. The checkpoint may be chosen after block 3, 4, 5, or 6 according to risk and architectural churn, but it must finish before a seventh block is allowed to extend that unchecked tranche. The reviewer checks that the train is converging on its intended architecture, that block boundaries and ownership remain coherent, and that the implementation conforms to the current DAG, charters, contracts, and all ordinary reviewed amendments effective for the train. Material findings are fixed through the owning block or an ordinary amendment, and affected conformance is rerun before the train proceeds.

When implementing the train's final intended block, the manager also spawns a fresh independent train review against the cumulative implementation, including that final candidate. It checks that every currently intended train outcome is implemented, integrated, and verified as amended. This review is additional to the final block's own review profile and to any Architect checkpoint due for the current tranche; it does not replace either. The final block cannot be accepted or landed until material train-review findings are resolved and the affected cumulative review passes.

These reviews are agent obligations, not implementation state. Their reports may remain ordinary task artifacts; there is no checkpoint row, amendment digest, receipt, or new readiness input.

## Trust model

Agents are trusted to transition rows to implemented only for implemented nodes and to provide useful locator hints. The simplicity is intentional. Do not add commit lookup, exact matching, SHA fallback, ancestry checks, GitHub verification, or a parallel receipt database.
