---
name: multi-agent-orchestration
description: >-
  Drive a substantial implementation or migration through bounded implementation,
  risk-scaled fresh review, consolidated fixes, verification, landing, and cleanup.
---

# Multi-Agent Orchestration

Use one parent orchestrator to own ordering, scope, review, landing, and cross-train coordination. A train manager coordinates only its named train; it does not use one shared mutation worktree for all of the train's blocks. Each independently landable node/block has its own implementation owner and candidate. Use subagents for concrete bounded implementation or fresh review work when the task calls for multi-agent execution.

## Tama readiness and the database-owned DAG

The program DAG is owned by the TAMA controller's database. Nodes, predecessors, charters, contracts, plans, decision records, issue mappings and implementation state all live there; the repository carries none of it. There is no `roadmap/` directory, no implementation ledger file, no repository-side DAG validator and no CI roadmap lane, and none may be reintroduced by a node's patch.

Readiness is intentionally simple. A node is implemented when the controller records its merge; a dispatchable node is READY when every transitive DAG ancestor is implemented. No activation, conditional, or in-progress state participates, and nothing in the repository is consulted to derive it.

An implementation patch therefore never edits DAG state. It does not add, move or rewrite charters, contracts, plans, decision records, ledgers or node tables in the tree, and it does not ship a per-node "constitution" or "ratification" harness that loads a repository DAG. A documentation or contract node lands its durable text where the code's own documentation lives (`docs/`, package READMEs, skill files) and records the ratified contract as a DAG asset through the controller. Landing the reviewed PR is what marks the node implemented.

GitHub is the landing path, not a mirror. Issue identity for a node, when one exists, comes from the controller's mapping, and the controller's own GitHub engine opens, labels and closes issues and pull requests. The repository's `scripts/githubctl` keeps only the offline `doctor` and the ruleset `protection` commands; issue sync, scheduling, project status, release planning and ledger writes are not repository concerns.

Landed charters are immutable historical acceptance records. Never retrofit this operating policy into a charter whose node is already implemented; update the owning active contract and orchestration policy through the controller instead.

When a user or maintainer directs DAG work to land without a GitHub PR, put one exact `Closes #<n>` line per controller-mapped issue in the final squash commit body before review. The issue closes when that commit reaches the origin default branch. Never put this coordination citation in source or tests.

If an existing GitHub issue must become DAG work, a maintainer authors the node and its charter in the controller. Never generate, propose, import, or apply DAG authority from GitHub or from repository files automatically.

## Admission and scope

Before implementation, confirm the node is READY, read its packet and charter, enumerate independently landable outcomes, and select proportionate evidence for every acceptance outcome. Split work that combines unrelated authority changes or independently rollbackable concerns. Tests are evidence, not quota; behavioral changes use TDD.

Production LOC and file budgets are planning references, not hard lines. Compare the actual candidate with them and investigate material drift in either direction. If a charter expects one production file and the candidate changes ten, treat that as a scope smell requiring a coherent explanation and a check for hidden independently landable work; do not reject, pad, or split a coherent implementation merely to hit the estimate. `rescope_loc` and `rescope_files` are stronger investigation signals under the same judgment-based rule.

Conflict domains, resources, external requirements, and effort fields are planning instructions. They are not leases or machine-validated authorizations. The maintainer coordinates ownership and ordering.

## Implementation and worktrees

The default landing unit is one independently landable node/block. Give it one dedicated branch/worktree, one stable candidate patch, one squash commit, and—when GitHub control is active—one mapped issue and one PR. Implement only that node's authorized scope, run targeted evidence, rebase as needed, and squash to one conventional commit before final review; the patch carries no DAG state. A train manager coordinates ordering and cross-node dependencies; it does not accumulate sibling-node changes in a shared mutation worktree.

Use one shared branch/worktree for multiple nodes only when the user or maintainer explicitly requests a single atomic train landing before mutation begins. Record why the nodes are not independently landable and keep the combined candidate reviewable as one unit; the controller marks every included node implemented when the candidate lands. Convenience, fewer PRs, shared files, or membership in the same named train are not sufficient reasons. Without that explicit exception, never mix independently landable nodes in one worktree, branch, squash, or PR.

One implementation or fix owner mutates a node candidate at a time. Additional implementation agents may work concurrently only in separate worktrees with disjoint landing units; reviewers and verifiers are read-only against a stable candidate. Do not add receipt files, candidate manifests, runtime state, or SHA-bound evidence.

Roadmap identity stays out of landed code and tests. Production file/module names and comments, plus all test file/module/test names, comments, fixtures, snapshots, assertion messages, and guard diagnostics, must describe durable behavior, never the program, roadmap/DAG, node/block/train ID, phase/stage, implementation sequence, or deletion history. A GitHub issue citation is allowed only for a specific independently reported defect outside the DAG-controlled issue mappings, and only alongside the durable behavioral explanation. Never cite the node's mapped issue or PR as code/test rationale.

In a fresh worktree, run `pnpm install --frozen-lockfile` before JS/TS tests or workspace-importing Node scripts so missing gitignored dependencies do not look like regressions.

Reviewers should inspect one stable node candidate patch, or the explicitly approved atomic multi-node candidate. The trust model does not require machine enforcement of immutability. Any material fix invalidates affected review conclusions by judgment; rerun the relevant review and verification without restamping identities.

## Train-wide conformance

The train manager keeps a human coordination count of newly implemented blocks since the previous train architecture checkpoint. After every 3 to 6 blocks, spawn a fresh Codex Architect conformance task over the cumulative train implementation. Select the checkpoint after block 3, 4, 5, or 6 based on risk and architectural churn, but complete it before a seventh unchecked block proceeds. Check convergence on the train's intended architecture, block and ownership coherence, and conformance to current DAG authority, charters, contracts, and every ordinary reviewed amendment effective for the train. Resolve material findings through the owning candidate or an ordinary amendment and rerun affected conformance before continuing.

On the train's final intended block, also spawn a fresh independent train-review task over all implemented train blocks plus the final candidate. It verifies that the full amended train intent is implemented, integrated, and evidenced. This cumulative review is additional to the final block's risk-scaled review and to any Architect checkpoint due for the current tranche. Do not accept or land the final block until material findings are resolved and the affected train review passes.

The checkpoint count and review reports remain ordinary coordination artifacts. Do not add implementation-ledger transitions, receipts, amendment digests, or readiness state for them.

## Risk-scaled review

- Low/simple: one fresh adversarial reviewer.
- Medium: adversarial plus an optional conformance lens when the profile calls for it.
- High/critical: three fresh tasks—adversarial, conformance, and a context-specific specialist.

Reviewers inspect the cumulative patch, proof selection, applicable tests, scope completeness, fail-closed behavior, performance implications, and architecture conformance. The author does not review its own work.

Consolidate all findings once per round. One fix agent addresses the full set and class-wide siblings. Add a regression only for a plausible boundary not already discriminated. Two review/fix cycles are the soft maximum; outside the scheduled train-conformance role above, use a neutral Architect only for real unresolved architecture ambiguity or a justified continuation ruling.

## Verification, landing, and cleanup

Run targeted evidence during implementation and the owning final gate on the final candidate. Land by squash-merging the reviewed node PR through GitHub; the TAMA controller records the merge and updates the node's issue. For an authorized non-PR landing, verify the reviewed squash commit body has every required mapped-issue closing line, then use the repository's normal landing workflow; pushing or merging that commit to the origin default branch performs issue closure. There is no fast-forward identity, landing receipt, activation command, or confirmation manifest.

Remove disposable worktrees after their results are recorded. Report the implemented node, commit locator hints, review verdicts, verification results, remaining limitations, and cleanup state.

See `references/templates.md` for prompts.

Autonomous train managers use the controller's default cumulative checkpoint policy: normally three new blocks, a hard six-block unchecked bound including active reservations, conservative first-adoption coverage and durable operational progress. A final train review must inspect the actual candidate checkout and cannot be waived by a node-review override. Changed candidate/baseline content requires a fresh affected review. These execution controls do not add a DAG readiness input.
