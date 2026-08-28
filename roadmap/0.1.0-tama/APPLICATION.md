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
2. Implement and run proportionate targeted verification.
3. Before squashing or review, add the node's ledger row to the implementation patch. Use the planned squash message, an approximate timezone-bearing date, and the PR number when known.
4. Squash once using that planned message.
5. Run the charter's fresh review profile against the squashed candidate and address findings.
6. Run the owning final gate and land the reviewed patch normally.

There is no after-commit ledger update, candidate finalization, receipt import, landing record, activation command, runtime root, or SHA restamping. If multiple nodes are deliberately implemented in one squash commit, each gets its own ledger row and the rows may share the same locator hints.

## GitHub flow after GH6

The ruling and historical reason for replacing GitHub-side DAG metadata are recorded in `decisions/2026-08-28-minimal-github-issue-mapping.md`.

Run `githubctl sync-issues` for initial issue creation, when explicitly adding more trains/nodes, or when a selected block is rescoped or its content changes. This flow is one-way from the local DAG/charter to GitHub. It creates missing issues and writes separate `[[github_issue]]` mappings with `sync_to_github = true`; commit that local mapping patch. For an already mapped opt-in node, the same explicit run updates the existing issue title/body in place while preserving its number, comments, and discussion. A `false` mapping is reported and skipped. The command never imports GitHub edits into local authority and does not continuously reconcile issues.

For implementation, resolve the mapped issue locally and create the PR with the expected final conventional-commit title. Put `Closes #<gh_issue>` in the PR body so GitHub links the issue and closes it only when the PR merges. For an opt-in mapping, place the useful description on the issue, end it with `Model: <model name>`, and omit effort and DAG metadata. For a protected mapping, keep the same closing link but do not edit the issue. At the end, before squash and final review, the agent completes the implementation row with the planned message, approximate timezone-bearing date, and known PR number.

To represent an existing GitHub issue in the DAG, manually author the node, charter, and `[[github_issue]]` row with `sync_to_github = false` in the same reviewed patch. No sync command imports or generates those local authority changes, and the existing issue remains protected from rewrite.

## Corrections

The ledger is trusted documentation. If a locator hint is unhelpful, correct it with an ordinary patch. The correction does not reopen, invalidate, or re-prove the node. Removing a row deliberately marks the node unimplemented and may block descendants.
