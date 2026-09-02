# Applying the Tama roadmap

The decision and historical reason for replacing ORC0's Git-identity control plane are recorded in `decisions/2026-08-28-trusted-implementation-ledger.md`.

## The only implementation state

`authority/state/implemented.toml` is the implementation ledger. Every DAG node is predeclared exactly once under `[implementation]`. A node is implemented when that line is transitioned from pending to implemented:

```toml
"D1" = { status = "implemented", commit_message = "refactor(core): move flow analysis into the semantic graph", commit_date = "2026-09-03T14:20:00+01:00", pull_request = 1234 }
```

Until then the same predeclared line is `"D1" = { status = "pending" }`. `pull_request` is optional. `commit_message` should normally be the planned squash subject or another useful search phrase. `commit_date` is an approximate ISO timestamp with timezone. Neither needs to match Git exactly. The transitioned row is the implementation fact.

The row itself is authoritative. Tooling never looks up the commit, compares the message or date, contacts GitHub, validates content, checks ancestry, or records a SHA. If several commits match a message search, use the date to choose the closest one; use the PR number when available.

GitHub issue numbers use a separate table in the same file:

```toml
[[github_issue]]
node_id = "D1"
gh_issue = 1234
sync_to_github = true
```

This is a direct local lookup plus an explicit local mutation policy. It lets `programctl github-issue 1234` find `D1`, but its presence does not mean D1 is implemented. `sync_to_github = true` permits one-way issue refresh. Use `false` when manually mapping a pre-existing issue that the project must not rewrite. The post-GH issue-sync command creates opt-in mappings when it creates issues.

Train-parent issue numbers use their own coordination table:

```toml
[[github_train_issue]]
train = "compiler.compiler-bridge"
gh_issue = 104
```

This row identifies the native GitHub parent for all active mapped blocks in that train. It is not an implementation row or a DAG node. Issue numbers are unique across node and train mappings.

## Readiness

A node is READY when:

- it is dispatchable;
- every transitive DAG ancestor is implemented.

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
2. Create a dedicated branch/worktree for that independently landable node. Do not mix sibling-node changes into it. When GitHub control is active, resolve the node's issue mapping before mutation. For an opt-in mapping, run `githubctl project-status --apply --node <ID> --status in-progress`, including when the work will stay local until landing; a protected mapping remains maintainer-owned. A shared train worktree is allowed only when the user or maintainer explicitly requested one atomic multi-node landing before mutation and the candidate records why the included nodes cannot land independently.
3. Implement and run proportionate targeted verification. When GitHub control is active, push the first implementation commit and then open a draft PR; keep that PR as the reviewed candidate and eventual landing path.
   Keep the roadmap out of landed source and tests: production file/module names and comments, plus test file/module/test names, comments, fixtures, snapshots, assertion messages, and guard diagnostics, describe durable behavior, never a DAG/node/train/phase or DAG-managed issue/PR. Only an independently reported non-DAG GitHub defect may be cited supplementally.
   Treat production LOC and file budgets as planning references, not hard limits. Compare the candidate with the estimates and investigate material drift. For example, one expected production file becoming ten is a scope smell that needs a coherent explanation, but the number alone neither rejects the patch nor requires padding or splitting it.
4. Before squashing or review, transition the node's predeclared ledger line from pending to implemented in the implementation patch. Use the planned squash message, an approximate timezone-bearing date, and the PR number when known.
5. Squash once using that planned title. For a user- or maintainer-directed non-PR landing, resolve each included node's local issue mapping and add one `Closes #<gh_issue>` line per node to the squash commit body before review.
6. Run the charter's fresh review profile against the squashed candidate and address findings.
7. Run the owning final gate. When GitHub control is active, land by squash-merging the reviewed node PR through GitHub; for an opt-in mapping, `squash-land` marks the mapped issue Done after merge and rolls its native parent only when all locally mapped train children are Done. Protected mappings remain maintainer-owned. For an authorized non-PR landing, verify every mapped closing line in the reviewed squash commit body, use the normal repository workflow, and run `project-status --apply --node <ID> --status done` for each opt-in mapping after the commit reaches the origin default branch.

There is no after-commit ledger update, candidate finalization, receipt import, landing record, activation command, runtime root, or SHA restamping. Multiple nodes may share one worktree, squash commit, and PR only under the explicit atomic-train exception above; each included node still transitions its own predeclared line and the lines may share the same locator hints.

## Train-level conformance

Keep a human coordination count of newly implemented blocks since the train's last architecture checkpoint. After every 3 to 6 blocks, spawn a fresh Codex Architect conformance review over the cumulative train implementation. Choose the checkpoint after block 3, 4, 5, or 6 based on risk and architectural churn, and complete it before a seventh unchecked block proceeds. Review the implementation against the train's intended architecture and the current DAG, charters, contracts, and ordinary reviewed amendments. Resolve material findings and rerun the affected conformance lens before continuing.

When the current candidate is the train's final intended block, also spawn a fresh independent train review over all implemented blocks plus the final candidate. It verifies that the complete amended train intent is implemented, integrated, and covered. This is additional to the block's own review profile and any Architect checkpoint due for the tranche. Do not accept or land the final block until material findings are resolved and the cumulative train review passes.

The checkpoint count and reports are ordinary coordination artifacts, not DAG state. They add no implementation-ledger transitions, receipts, amendment digests, or readiness inputs.

## GitHub flow after GH6

The ruling and historical reason for replacing GitHub-side DAG metadata are recorded in `decisions/2026-08-28-minimal-github-issue-mapping.md`.

GH and REL charters with implementation-ledger rows are frozen historical acceptance records. Current GitHub operations follow `contracts/github-control-plane.md`; amend that contract and this guide rather than rewriting landed charters.

Run `githubctl sync-issues` for initial issue creation, when explicitly adding more trains/nodes, or to reconcile the versioned label and milestone catalogs, direct blocked-by edges, explicit `gh_milestone` assignments, managed labels, train parents, native sub-issues, and Project 3 membership. Before initial creation or explicit `--refresh-content`, author or update the selected node's stable `catalogs/github-issue-content.toml` entry from the charter and current source. Every active train also needs one reviewed `catalogs/github-train-issues.toml` entry containing its parent title, problem, outcome, acceptance, problem label, and completion-horizon milestone. Both renderers follow the human issue standard from `contracts/github-control-plane.md`, never extract charter sections, and end bodies with `AI-Generated` without a model name. Missing or invalid content aborts before GitHub mutation.

A selection that depends on unresolved nodes outside its boundary fails without mutation and reports the required nodes. Pass `--create-blockers` to include that predecessor closure and create any missing issues, or `--ignore-blockers` to keep the requested boundary and leave those relationships untouched. Do not pass both. Automatic blocker expansion never includes a locally complete predecessor. Locally completed/history nodes and closed GitHub issues are no-touch even when explicitly named. The command writes separate `[[github_issue]]` mappings with `sync_to_github = true` for newly created children and `[[github_train_issue]]` mappings for newly created parents; commit that local mapping patch. For an already mapped opt-in child or parent, a normal run leaves title/body untouched. Use `--refresh-content` only after a rescope or deliberate content change. A `false` child mapping is reported and skipped without reading or mutating the issue. The command never imports GitHub edits into local authority and does not run continuously.

Every active train has one mapped parent GitHub issue, and each independently landable active block is its native sub-issue. `githubctl sync-issues` creates a missing parent, stores its returned number, applies deterministic area/problem/framework/AI labels and its train milestone, attaches the active children, and ensures parent and children are members of Project 3. Every newly added Project item is initialized to Todo; an existing Todo, In Progress, or Done status is preserved. A child attached to another parent aborts the complete sync before mutation. GitHub's 100-child limit is a hard signal to split an oversized train into reviewable trains rather than disguise a larger train as one block. Project lifecycle uses locally mapped active nodes in the train as the expected child set, so GitHub omissions cannot mark the train Done early.

For implementation, resolve the mapped issue locally and create the node's dedicated branch/worktree. Schedule an opt-in mapping and its eligible native parent into Project 3, then mark it In Progress before mutation; a protected mapping receives no Project read or write. After the first implementation commit is pushed, open a draft PR with the expected final conventional-commit title. The reviewed PR is the landing path and is squash-merged through GitHub: do not land locally first and mirror the result afterward. Put `Closes #<gh_issue>` in the PR body so GitHub links the issue and closes it only when the PR merges. For an opt-in mapping, preserve useful issue prose, end it with `AI-Generated`, and omit effort and DAG metadata. For a protected mapping, keep the same closing link but do not edit the issue. At the end, before squash and final review, the agent completes the implementation row with the planned message, approximate timezone-bearing date, and known PR number.

If the user or maintainer explicitly selects a non-PR landing instead, keep the same local issue identity but put each included node's `Closes #<gh_issue>` line in the final squash commit body rather than a PR body. The reviewed commit is then pushed or merged through the normal repository workflow. GitHub closes the issues only when it reaches the origin default branch; mark each opt-in node Done in Project 3 only after that point, while protected mappings remain maintainer-owned.

To represent an existing GitHub issue in the DAG, follow `ManualDagAuthoring`: manually author the node, charter, and `[[github_issue]]` row with `sync_to_github = false` in the same reviewed patch. No sync command imports or generates those local authority changes, and the existing issue remains protected from rewrite.

## Corrections

The ledger is trusted documentation. If a locator hint is unhelpful, correct it with an ordinary patch. The correction does not reopen, invalidate, or re-prove the node. Flipping the row back to `status = "pending"` is the deliberate operation that marks a node unimplemented and may block descendants.
