# Orchestration contract

## State authority

The static DAG defines nodes and predecessor edges. `authority/state/implemented.toml` defines which nodes are implemented. Row presence is the complete lifecycle fact.

Each row carries:

- `node_id` — the DAG node;
- `commit_message` — a useful, non-exact commit search phrase;
- `commit_date` — an approximate timezone-bearing date used to choose the closest result when a phrase finds several commits;
- `pull_request` — an optional PR number, particularly useful after GH0.

The locator fields are never validated. They do not prove content, authorship, review, ancestry, landing, or identity.

## Readiness algorithm

For each node:

1. An implemented row makes the node COMPLETE.
2. No DAG status, Git state, or historical record makes a node COMPLETE without that row.
3. A dispatchable node becomes READY when every transitive DAG ancestor has an implemented row.
4. Other nodes are BLOCKED.

Nothing else affects readiness. There is no activation gate, conditional-predecessor state, or declared in-progress status. External requirements, conflict domains, resources, gates, and review profiles are instructions followed by agents and maintainers.

## Candidate workflow

Each independently landable node is one candidate by default: one dedicated worktree and branch, one implementation owner, one squash commit, and—when GitHub control is active—one mapped issue and one pull request. A train manager coordinates ordering but does not accumulate sibling-node mutations in one shared worktree. Multiple nodes may share a worktree, branch, squash, or pull request only when the user or maintainer explicitly requests one atomic multi-node landing before mutation and records why those nodes cannot land independently.

The implementer adds the node's ledger row to its implementation patch before squash or review. The row names the planned squash title and approximate date. When GitHub control is active, the issue mapping and node worktree/branch exist before mutation, and a draft pull request is opened after the first implementation commit is pushed. The candidate is then squashed once, reviewed according to its profile, and verified according to its gate profile. The reviewed pull request is the landing path and is squash-merged through GitHub; the candidate is not landed locally first and mirrored afterward. For a user- or maintainer-directed non-PR landing, the final squash commit body instead contains one `Closes #<gh_issue>` line for every included node's local mapping before review; normal landing to the origin default branch then performs issue closure.

The candidate's landed source and tests contain no roadmap identity: no program/revision, DAG, node/block/train ID, phase/stage, implementation sequence, deletion history, or DAG-managed issue/PR citation. Comments and test artifacts state durable behavior. A GitHub issue may be cited only for a specific independently reported defect outside the DAG mappings, and only as a supplement to that behavioral explanation.

The lifecycle has no admission, lease, dispatch receipt, candidate-finalization receipt, review manifest, acceptance receipt, landing receipt, activation transition, amendment digest chain, or runtime root. Review reports may remain ordinary task artifacts; they are not machine-bound to Git identities.

## Trust model

Agents are trusted to add rows only for implemented nodes and to provide useful locator hints. The simplicity is intentional. Do not add commit lookup, exact matching, SHA fallback, ancestry checks, GitHub verification, or a parallel receipt database.
