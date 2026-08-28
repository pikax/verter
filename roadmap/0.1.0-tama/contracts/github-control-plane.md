# Minimal GitHub workflow contract

GH0 may begin when every transitive ancestor has an implemented-ledger row. Every later GH, FB, and REL node uses the same rule.

The static DAG and local ledger remain authority. GitHub is a human coordination surface. Issues and PRs do not carry or validate serialized DAG state.

## GitHubIssueMapping

The ledger may contain mappings that are separate from implementation rows:

```toml
[[github_issue]]
node_id = "D1"
gh_issue = 1234
sync_to_github = true
```

`GitHubIssueMapping` identity is exactly `{node_id, gh_issue}`, unique both ways. Titles, labels, GitHub state, and other issue prose are not identity. The required `sync_to_github` boolean is a local mutation policy on that identity, never a uniqueness key and never readiness: `true` permits the one-way DAG/charter-to-GitHub refresh, while `false` protects a pre-existing issue that was manually mapped into the DAG. A GitHub issue does not need a DAG marker, hidden comment, managed region, predecessor list, effort tier, state label, or other generated metadata. From a node, read `gh_issue`; from an issue number, search the same table with `programctl github-issue`. `programctl github-issues` lists the three stored fields (`node_id`, `gh_issue`, `sync_to_github`) sorted by `node_id`. A `[[github_issue]]` row never marks the node implemented.

## GitHubIssueDescription

An opt-in issue is ordinary human-readable work context. For synchronization, its title comes from the selected node name and its work description comes from the live charter's current outcome, scope, forbidden designs, and abort conditions. The metadata header, predecessor/readiness fields, effort/budget fields, and transferred historical source are excluded. The creating, synchronizing, or implementing agent ends the body with exactly one model line:

```text
Model: <model name>
```

Do not include effort, reasoning tier, DAG ID, predecessors, readiness, generated labels, machine markers, or a metadata block in an opt-in issue body. An explicit rescope sync updates the opt-in issue's title and complete body while retaining or replacing the final model line as requested. A protected issue is left byte-for-byte untouched by this workflow. Titles and prose are not identity; the local `gh_issue` mapping is.

## ExpectedPullRequestTitle

When implementation begins, the agent creates the PR with the expected final conventional-commit title. Its body contains GitHub's ordinary closing link, `Closes #<gh_issue>`, using the exact local mapping. This attaches the issue to the PR and lets GitHub close it only when that PR merges; an abandoned or closed-without-merge PR leaves the issue open. The closing link is required for both mapping policies. For `sync_to_github = true`, the agent puts the useful implementation description and final model line on the issue. For `false`, the agent does not edit the issue body or title because the project did not author it. The PR may carry normal review and validation prose, but no serialized DAG metadata or effort block is required.

Before squash and review finish, the completing agent adds or updates the node's `[[implemented]]` row with the planned final `commit_message`, approximate timezone-bearing `commit_date`, and known `pull_request`. The row is part of the implementation patch. The PR number remains an unvalidated locator, and no post-merge reconciliation or SHA restamping follows.

## GitHubIssueSync

After the GH train lands, `githubctl sync-issues` owns the occasional explicit one-way synchronization run from local DAG/charter authority to GitHub. It accepts a named train or node set. In check mode it reports missing mappings and `sync_to_github = true` issues whose ordinary title or work description no longer matches the selected block after a rescope or content change. In apply mode it creates missing issues, writes their returned mappings with `sync_to_github = true`, and updates the title/body of already mapped opt-in issues in place. A `sync_to_github = false` row is lookup-only: check/apply reports it as protected and never rewrites that issue. The issue number, comments, and discussion history remain intact. An update replaces ordinary work prose only; it never imports GitHub title/body/labels/state into the DAG or ledger, adds DAG metadata, or infers identity from prose. This command is run for initial issue creation, later train/node additions, or an explicit rescope refresh—not as continuous reconciliation.

## Trust and ownership

Agents are trusted to use the correct issue and PR mappings. Tooling may check local structural uniqueness, verify that a created PR body contains exactly the mapped closing link, and compare a selected mapped issue with the locally rendered description, but it never treats GitHub as authority or imports GitHub changes. Given an issue number, reverse lookup is only a search in the local `gh_issue` table; it is not reverse synchronization. GitHub closure, labels, milestones, checks, and Project fields never satisfy DAG ancestors or mark implementation complete.

P0/P1 findings block under the owning review policy. Lower-severity findings follow that policy and may be coordinated in ordinary issue or PR prose. Do not build a second receipt, lifecycle, managed-body, marker, or continuous bidirectional synchronization system.

If a maintainer wants an existing GitHub issue represented in the DAG, the maintainer manually authors the DAG node, charter, and a `[[github_issue]]` row with `sync_to_github = false` in one ordinary reviewed patch. No GitHub command or issue field generates, proposes, imports, or applies local DAG authority, and later sync runs must not rewrite that issue because the project did not author it.
