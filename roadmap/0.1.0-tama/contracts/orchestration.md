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

The implementer adds the ledger row to the implementation patch before squash or review. The row names the planned squash message and approximate date. The candidate is then squashed once, reviewed according to its profile, verified according to its gate profile, and landed normally.

The lifecycle has no admission, lease, dispatch receipt, candidate-finalization receipt, review manifest, acceptance receipt, landing receipt, activation transition, amendment digest chain, or runtime root. Review reports may remain ordinary task artifacts; they are not machine-bound to Git identities.

## Trust model

Agents are trusted to add rows only for implemented nodes and to provide useful locator hints. The simplicity is intentional. Do not add commit lookup, exact matching, SHA fallback, ancestry checks, GitHub verification, or a parallel receipt database.

