---
name: multi-agent-orchestration
description: >-
  Drive a substantial implementation or migration through bounded implementation,
  risk-scaled fresh review, consolidated fixes, verification, landing, and cleanup.
---

# Multi-Agent Orchestration

Use one parent orchestrator to own ordering, scope, review, landing, and cross-train coordination. A train manager coordinates only its named train; it does not use one shared mutation worktree for all of the train's blocks. Each independently landable node/block has its own implementation owner and candidate. Use subagents for concrete bounded implementation or fresh review work when the task calls for multi-agent execution.

## Tama readiness and ledger

Tama readiness is intentionally simple. `roadmap/0.1.0-tama/authority/state/implemented.toml` is the only implementation ledger. A row means the node is implemented. A dispatchable node is READY when every transitive DAG ancestor has a row; no activation, conditional, or in-progress state participates.

Each row records the node ID, a useful commit-message search phrase, an approximate timezone-bearing date, and optionally a PR number. Those values are locator hints only. Never resolve or validate them, require exact matching, compare Git SHAs or trees, inspect ancestry, contact GitHub, or create a parallel receipt database.

The node's implementation patch adds its ledger row before squash or review, using the planned squash message and approximate date. Then squash once and review that node's resulting candidate. No post-commit ledger update is required.

When the GitHub control-plane workflow is active, GitHub is the implementation landing path, not a mirror of work landed locally first. Child issue identity comes from separate `[[github_issue]]` rows in the same ledger file. They map `node_id` to `gh_issue`, require `sync_to_github`, and never mark implementation complete. `true` permits one-way issue refresh; `false` protects a pre-existing issue from every agent-driven GitHub mutation. Each active train also has one `[[github_train_issue]]` coordination identity; its active mapped blocks are native GitHub sub-issues. Use `githubctl sync-issues` for initial mapping, explicit later train additions, deterministic label/milestone/dependency/parent/Project reconciliation, or an explicit refresh after a block or train rescope/content change. Sync creates missing train parents, attaches native sub-issues, and adds parent and children to Project 3; every newly added item is initialized to Todo, while every existing status is preserved. Completed/history nodes and closed issues are no-touch. A child already attached to another parent aborts before mutation. GitHub's 100-child limit counts every native child already attached, including completed or manual work, so a train whose projected membership exceeds it must be split. A refresh updates only opt-in issue prose and preserves issue number and discussion; protected issues are reported and skipped, and GitHub edits never flow back. Before creation or explicit refresh, author or update the node's stable `catalogs/github-issue-content.toml` entry and the train's stable `catalogs/github-train-issues.toml` entry once from the charter and current source. They must satisfy `contracts/github-control-plane.md`'s human issue standard: a standalone `Problem`, `Expected outcome`, and three-to-six-bullet `Acceptance` description that never copies charter sections, program/DAG prose, abort conditions, budgets, gates, or generic boilerplate. Catalog renderers end generated issue prose with exactly `AI-Generated`; never store a model name. Normal sync does not regenerate existing prose. Before mutation, resolve the node's mapping and create its branch/worktree. For an opt-in mapping, ensure the issue and parent are scheduled, then run `githubctl project-status --apply --node <ID> --status in-progress`, including local-only work; a protected mapping is maintainer-owned and receives no Project command. After the first implementation commit is pushed, open the node's draft PR with the expected final title, link the mapped issue, keep that PR as the reviewed candidate, and land by squash-merging it through GitHub. The finishing agent records the known PR number in the same-patch implementation row before squash and final review. Do not bypass the PR by landing the candidate locally and syncing GitHub afterward.

Landed charters are immutable historical acceptance records. Never retrofit this operating policy into a charter whose node already has an implementation-ledger row; update the owning active contract, application guide, and orchestration policy instead.

When a user or maintainer directs DAG work to land without a GitHub PR, every included node still resolves its local `[[github_issue]]` mapping. Put one exact `Closes #<gh_issue>` line per included node in the final squash commit body before review. For the default one-node candidate that is one closing line; an explicitly approved atomic multi-node candidate carries one line for every mapped node. The issue closes when that commit reaches the origin default branch. Only then run `githubctl project-status --apply --node <ID> --status done` for each opt-in landed node; protected mappings remain maintainer-owned. Never put this coordination citation in source or tests.

If an existing GitHub issue must become DAG work, a maintainer manually authors the DAG node, charter, and `[[github_issue]]` row with `sync_to_github = false` in one reviewed patch. Never rewrite that issue or generate, propose, import, or apply DAG authority from GitHub automatically.

## Admission and scope

Before implementation, confirm the node is READY, read its packet and charter, enumerate independently landable outcomes, and select proportionate evidence for every acceptance outcome. Split work that combines unrelated authority changes or independently rollbackable concerns. Tests are evidence, not quota; behavioral changes use TDD.

Production LOC and file budgets are planning references, not hard lines. Compare the actual candidate with them and investigate material drift in either direction. If a charter expects one production file and the candidate changes ten, treat that as a scope smell requiring a coherent explanation and a check for hidden independently landable work; do not reject, pad, or split a coherent implementation merely to hit the estimate. `rescope_loc` and `rescope_files` are stronger investigation signals under the same judgment-based rule.

Conflict domains, resources, external requirements, and effort fields are planning instructions. They are not leases or machine-validated authorizations. The maintainer coordinates ownership and ordering.

## Implementation and worktrees

The default landing unit is one independently landable node/block. Give it one dedicated branch/worktree, one stable candidate patch, one squash commit, and—when GitHub control is active—one mapped issue and one PR. Implement only that node's authorized scope, add its implementation-ledger row to the patch, run targeted evidence, rebase as needed, and squash to one conventional commit before final review. A train manager coordinates ordering and cross-node dependencies; it does not accumulate sibling-node changes in a shared mutation worktree.

Use one shared branch/worktree for multiple nodes only when the user or maintainer explicitly requests a single atomic train landing before mutation begins. Record why the nodes are not independently landable, keep the combined candidate reviewable as one unit, and give every included node its own ledger row. Convenience, fewer PRs, shared files, or membership in the same named train are not sufficient reasons. Without that explicit exception, never mix independently landable nodes in one worktree, branch, squash, or PR.

One implementation or fix owner mutates a node candidate at a time. Additional implementation agents may work concurrently only in separate worktrees with disjoint landing units; reviewers and verifiers are read-only against a stable candidate. Do not add receipt files, candidate manifests, runtime state, or SHA-bound evidence.

Roadmap identity stays out of landed code and tests. Production file/module names and comments, plus all test file/module/test names, comments, fixtures, snapshots, assertion messages, and guard diagnostics, must describe durable behavior, never the program, roadmap/DAG, node/block/train ID, phase/stage, implementation sequence, or deletion history. A GitHub issue citation is allowed only for a specific independently reported defect outside the DAG-controlled issue mappings, and only alongside the durable behavioral explanation. Never cite the node's mapped issue or PR as code/test rationale.

In a fresh worktree, run `pnpm install --frozen-lockfile` before JS/TS tests or workspace-importing Node scripts so missing gitignored dependencies do not look like regressions.

Reviewers should inspect one stable node candidate patch, or the explicitly approved atomic multi-node candidate. The trust model does not require machine enforcement of immutability. Any material fix invalidates affected review conclusions by judgment; rerun the relevant review and verification without restamping identities.

## Train-wide conformance

The train manager keeps a human coordination count of newly implemented blocks since the previous train architecture checkpoint. After every 3 to 6 blocks, spawn a fresh Codex Architect conformance task over the cumulative train implementation. Select the checkpoint after block 3, 4, 5, or 6 based on risk and architectural churn, but complete it before a seventh unchecked block proceeds. Check convergence on the train's intended architecture, block and ownership coherence, and conformance to current DAG authority, charters, contracts, and every ordinary reviewed amendment effective for the train. Resolve material findings through the owning candidate or an ordinary amendment and rerun affected conformance before continuing.

On the train's final intended block, also spawn a fresh independent train-review task over all implemented train blocks plus the final candidate. It verifies that the full amended train intent is implemented, integrated, and evidenced. This cumulative review is additional to the final block's risk-scaled review and to any Architect checkpoint due for the current tranche. Do not accept or land the final block until material findings are resolved and the affected train review passes.

The checkpoint count and review reports remain ordinary coordination artifacts. Do not add ledger rows, receipts, amendment digests, or readiness state for them.

## Risk-scaled review

- Low/simple: one fresh adversarial reviewer.
- Medium: adversarial plus an optional conformance lens when the profile calls for it.
- High/critical: three fresh tasks—adversarial, conformance, and a context-specific specialist.

Reviewers inspect the cumulative patch, proof selection, applicable tests, scope completeness, fail-closed behavior, performance implications, and architecture conformance. The author does not review its own work.

Consolidate all findings once per round. One fix agent addresses the full set and class-wide siblings. Add a regression only for a plausible boundary not already discriminated. Two review/fix cycles are the soft maximum; outside the scheduled train-conformance role above, use a neutral Architect only for real unresolved architecture ambiguity or a justified continuation ruling.

## Verification, landing, and cleanup

Run targeted evidence during implementation and the owning final gate on the final candidate. When GitHub control is active, land by squash-merging the reviewed node PR through GitHub; for an opt-in mapping, `squash-land` marks the issue Done only after merge and rolls its native parent when all locally mapped train children are Done. Protected mappings remain maintainer-owned. For an authorized non-PR landing, verify the reviewed squash commit body has every required mapped-issue closing line, then use the repository's normal landing workflow; pushing or merging that commit to the origin default branch performs issue closure, after which `project-status ... done` updates each opt-in Project 3 item. There is no fast-forward identity, landing receipt, activation command, or confirmation manifest.

Remove disposable worktrees after their results are recorded. Report the implemented node, commit locator hints, review verdicts, verification results, remaining limitations, and cleanup state.

See `references/templates.md` for prompts.
