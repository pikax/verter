# Applying the Tama roadmap

The decision and historical reason for replacing ORC0's Git-identity control plane are recorded in `decisions/2026-08-28-trusted-implementation-ledger.md`.

## The only implementation state

`authority/state/implemented.toml` is the implementation ledger. A row means the node is implemented:

```toml
[[implemented]]
node_id = "D1"
commit_message = "refactor(core): move flow analysis into the semantic graph"
commit_date = "2026-09-03T14:20:00+01:00"
pull_request = 1234
```

`pull_request` is optional. `commit_message` should normally be the planned squash subject or another useful search phrase. `commit_date` is an approximate ISO timestamp with timezone. Neither needs to match Git exactly.

The row itself is authoritative. Tooling never looks up the commit, compares the message or date, contacts GitHub, validates content, checks ancestry, or records a SHA. If several commits match a message search, use the date to choose the closest one; use the PR number when available.

GitHub issue numbers use a separate table in the same file:

```toml
[[github_issue]]
node_id = "D1"
gh_issue = 1234
sync_to_github = true
```

This is a direct local lookup plus an explicit local mutation policy. It lets `programctl github-issue 1234` find `D1`, but its presence does not mean D1 is implemented. `sync_to_github = true` permits one-way issue refresh. Use `false` when manually mapping a pre-existing issue that the project must not rewrite. The post-GH issue-sync command creates opt-in mappings when it creates issues.

## Readiness

A node is READY when:

- it is dispatchable;
- every transitive DAG ancestor has an `implemented` row.

No other lifecycle state is consulted. A recorded direct predecessor cannot hide a missing earlier ancestor. Conflict/resource/external-requirement fields are planning instructions for agents and maintainers, not locks or machine-validated authorizations.

The frontier command is read-only and stateless. It is just a convenient rendering of this rule; there is no start record or start commit. A node with no unimplemented ancestor can start immediately.

Inspect the frontier with:

```text
node roadmap/0.1.0-tama/tools/programctl.mjs frontier
node roadmap/0.1.0-tama/tools/programctl.mjs explain D1
node roadmap/0.1.0-tama/tools/programctl.mjs packet D1
```

## Implementation and review

1. Check that the node is READY and read its packet and charter.
2. Create a dedicated branch/worktree for that independently landable node. Do not mix sibling-node changes into it. When GitHub control is active, resolve the node's issue mapping before mutation. A shared train worktree is allowed only when the user or maintainer explicitly requested one atomic multi-node landing before mutation and the candidate records why the included nodes cannot land independently.
3. Implement and run proportionate targeted verification. When GitHub control is active, push the first implementation commit and then open a draft PR; keep that PR as the reviewed candidate and eventual landing path.
   Keep the roadmap out of landed source and tests: production file/module names and comments, plus test file/module/test names, comments, fixtures, snapshots, assertion messages, and guard diagnostics, describe durable behavior, never a DAG/node/train/phase or DAG-managed issue/PR. Only an independently reported non-DAG GitHub defect may be cited supplementally.
4. Before squashing or review, add the node's ledger row to the implementation patch. Use the planned squash message, an approximate timezone-bearing date, and the PR number when known.
5. Squash once using that planned title. For a user- or maintainer-directed non-PR landing, resolve each included node's local issue mapping and add one `Closes #<gh_issue>` line per node to the squash commit body before review.
6. Run the charter's fresh review profile against the squashed candidate and address findings.
7. Run the owning final gate. When GitHub control is active, land by squash-merging the reviewed node PR through GitHub. For an authorized non-PR landing, verify every mapped closing line in the reviewed squash commit body and then use the normal repository workflow; the issues close when that commit reaches the origin default branch.

There is no after-commit ledger update, candidate finalization, receipt import, landing record, activation command, runtime root, or SHA restamping. Multiple nodes may share one worktree, squash commit, and PR only under the explicit atomic-train exception above; each included node still gets its own ledger row and the rows may share the same locator hints.

## GitHub flow after GH6

The ruling and historical reason for replacing GitHub-side DAG metadata are recorded in `decisions/2026-08-28-minimal-github-issue-mapping.md`.

Run `githubctl sync-issues` for initial issue creation, when explicitly adding more trains/nodes, or when a selected block is rescoped or its content changes. This flow is one-way from the local DAG/charter to GitHub. It creates missing issues and writes separate `[[github_issue]]` mappings with `sync_to_github = true`; commit that local mapping patch. For an already mapped opt-in node, the same explicit run updates the existing issue title/body in place while preserving its number, comments, and discussion. The agent synthesizes the human issue standard from `contracts/github-control-plane.md`; it never copies charter sections or publishes program/DAG wording, abort conditions, budgets, gates, commands, or boilerplate. A `false` mapping is reported and skipped. The command never imports GitHub edits into local authority and does not continuously reconcile issues.

A named train may have one parent GitHub issue. Each independently landable block is a sub-issue of that parent. `githubctl sync-issues` maps and refreshes the block issues; it does not create the parent. Create the parent separately and attach the mapped block issues as sub-issues so the train stays one human coordination surface.

For implementation, resolve the mapped issue locally and create the node's dedicated branch/worktree before mutation. After the first implementation commit is pushed, open a draft PR with the expected final conventional-commit title. The reviewed PR is the landing path and is squash-merged through GitHub: do not land locally first and mirror the result afterward. Put `Closes #<gh_issue>` in the PR body so GitHub links the issue and closes it only when the PR merges. For an opt-in mapping, place the useful description on the issue, end it with `Model: <model name>`, and omit effort and DAG metadata. For a protected mapping, keep the same closing link but do not edit the issue. At the end, before squash and final review, the agent completes the implementation row with the planned message, approximate timezone-bearing date, and known PR number.

If the user or maintainer explicitly selects a non-PR landing instead, keep the same local issue identity but put each included node's `Closes #<gh_issue>` line in the final squash commit body rather than a PR body. The reviewed commit is then pushed or merged through the normal repository workflow, and GitHub closes the issues only when it reaches the origin default branch.

To represent an existing GitHub issue in the DAG, follow `ManualDagAuthoring`: manually author the node, charter, and `[[github_issue]]` row with `sync_to_github = false` in the same reviewed patch. No sync command imports or generates those local authority changes, and the existing issue remains protected from rewrite.

## Corrections

The ledger is trusted documentation. If a locator hint is unhelpful, correct it with an ordinary patch. The correction does not reopen, invalidate, or re-prove the node. Removing a row deliberately marks the node unimplemented and may block descendants.
