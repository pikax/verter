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

After the GH train lands, `githubctl sync-issues` owns the occasional explicit one-way synchronization run from local DAG/charter authority to GitHub. It accepts a named train or node set. In check mode it reports missing mappings and `sync_to_github = true` issues whose ordinary title or work description no longer matches the selected block after a rescope or content change. That check report is the named `IssueCreateOrUpdatePlan` (`missing`, `drift`, `protected`, `current`); apply executes the same plan and does not invent a second planner. In apply mode it creates missing issues, writes their returned mappings with `sync_to_github = true`, and updates the title/body of already mapped opt-in issues in place. Before every opt-in apply update it GETs the mapped issue through the same unambiguous read used in check (`parseIssuePayload`, including rejection of PR-shaped `pull_request != null`) and aborts `UnstructuredGitHubOutputError` before PATCH—including a GET 404. A create records `{node_id, gh_issue, kind, mapping_written: false}` as soon as GitHub returns a number; writing the local mapping flips `mapping_written`. Any later failure is `PartialFailureError` including that identity row even when it is the first operation. CLI identity is `node_id` / number / `mapping_written`, never title or body. A `sync_to_github = false` row is lookup-only: check/apply reports it as protected and never rewrites that issue. The issue number, comments, and discussion history remain intact. An update replaces ordinary work prose only; it never imports GitHub title/body/labels/state into the DAG or ledger, adds DAG metadata, or infers identity from prose. This command is run for initial issue creation, later train/node additions, or an explicit rescope refresh—not as continuous reconciliation.

## Trust and ownership

Agents are trusted to use the correct issue and PR mappings. Tooling may check local structural uniqueness, verify that a created PR body contains exactly the mapped closing link, and compare a selected mapped issue with the locally rendered description, but it never treats GitHub as authority or imports GitHub changes. Given an issue number, reverse lookup is only a search in the local `gh_issue` table; it is not reverse synchronization. GitHub closure, labels, milestones, checks, and Project fields never satisfy DAG ancestors or mark implementation complete.

P0/P1 findings block under the owning review policy. Lower-severity findings follow that policy and may be coordinated in ordinary issue or PR prose. Do not build a second receipt, lifecycle, managed-body, marker, or continuous bidirectional synchronization system.

If a maintainer wants an existing GitHub issue represented in the DAG, the maintainer manually authors the DAG node, charter, and a `[[github_issue]]` row with `sync_to_github = false` in one ordinary reviewed patch. No GitHub command or issue field generates, proposes, imports, or applies local DAG authority, and later sync runs must not rewrite that issue because the project did not author it.

## GitHubAdapter

All GitHub network effects go through `GitHubAdapter`. `programctl` remains local. The live adapter calls `gh api --include` and reads HTTP status from the structured header block (`HTTP/\d… <code>`), split from the JSON body on the first blank line (CRLF or LF). It never scrapes issue or pull-request URLs, stderr, terminal prose, or an optional JSON `status` field, and it does not classify from process exit status. Mutation methods require `mode: "check" | "apply"`. Check plans without writing. Apply requires a `GitHubDoctor` clearance minted for that adapter instance whose `owner`/`repo` match the adapter's bound repository. Owner and repo are set once at construction and are not rebindable. The network transport is constructor-injected and is not a public request port. Returned issue and pull-request numbers are JSON numbers for the caller to persist; this adapter does not write `[[github_issue]]` rows.

Issue update is a local-policy operation: a mapping with `sync_to_github === false` is refused without contacting GitHub. Opt-in updates replace title and body in place and preserve the issue number and comment history.

Pull-request creation requires the exact mapped closing link `Closes #<n>` in the body and returns the created number. Issue create, issue update, and PR create records carry `kind`, the returned `number` when applied, and `applied`.

## GitHubDoctor

`GitHubDoctor` is check-only. It validates authentication, repository access, and the issue and pull-request mutation capabilities required before a write. It never creates, updates, or labels resources and it never stores credentials. `inspectCapabilities` returns one `{authenticated, login?, repository, issues, pullRequests}` record. Expected misses (unauthenticated, missing repository, wrong `full_name`, missing issue write, missing pull-request write) do not throw; `check()` folds them into `{ok, errors, capabilities, clearance}`. Unauthenticated and missing-repository folds come from HTTP 401/403/404, not from JSON `status`. Unstructured output (missing HTTP status line or non-object JSON) is the only inspect failure that throws. Apply clearance is minted only by `check()` for that adapter instance; a hand-built `{kind, owner, repo, issues:true}` object is not clearance, and a minted clearance for a different owner/repo is not clearance.

## FakeGitHubAdapter

`FakeGitHubAdapter` implements the same methods as `GitHubAdapter` for tests. It assigns deterministic issue and pull-request numbers from one shared monotonic sequence, preserves comment lists across opt-in updates, records protected-mapping refusals without mutation, requires the exact `Closes #<n>` link, and reports partial failure by the numbers already returned. Check mode plans locally without existence lookup; apply owns 404 and duplicate. Live GitHub is not a test substrate.
