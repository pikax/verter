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

`githubctl create-pr --check|--apply --node <ID> --title <final conventional commit> --head <branch>` owns that start flow. Check plans; apply is doctor-gated for `issues` and `pullRequests` (Project 3 is not required). The PR body is ordinary prose plus exactly one `Closes #<n>` produced by `mappedClosingLink`; `Fixes`/`Close` and a second closing link are rejected. Apply never attaches Project 3. `--write-locator` sets `pull_request` on an existing `[[implemented]]` row only; it never invents a row or infers COMPLETE from PR state. If that row already locates a pull request, abort before issue update or PR create, independent of `--write-locator`. Missing mapping, a missing ancestor ledger row, an existing PR for the same head, or the wrong repository abort before PATCH/POST. One node, one PR. Check and apply list open pulls for that head through the shared adapter method `pullsForHead`.

Before squash and review finish, the completing agent adds or updates the node's `[[implemented]]` row with the planned final `commit_message`, approximate timezone-bearing `commit_date`, and known `pull_request`. The row is part of the implementation patch. The PR number remains an unvalidated locator, and no post-merge reconciliation or SHA restamping follows.

## ReviewCycleSummary

`githubctl review-summary --check|--apply --pr <n> --node <ID> --verdict PASS|FAIL --body <human prose>` records one human-written review cycle on the mapped pull request as an ordinary issue comment. The named `ReviewCycleSummary` covers problem, scope, validation, and review in ordinary prose. It is not a managed region and does not carry DAG metadata, effort, a commit SHA, or a digest binding.

Check plans. Apply is doctor-gated for `issues` and `pullRequests` (Project 3 is not required) and posts through structured `gh api` JSON (`POST /repos/{owner}/{repo}/issues/{n}/comments`). Apply never attaches Project 3, never closes an issue, never writes `[[implemented]]` rows, and never treats closing or editing an issue as finding resolution.

The pull request must contain the exact mapped `Closes #<n>` link or be the node's located `pull_request`. A wrong mapping aborts before comment or issue mutation.

For `sync_to_github = true`, apply ensures the issue description ends with exactly one `Model: <model name>` line, stripping extra model lines and effort fields without replacing human prose. Protected mappings (`sync_to_github = false`) record the review on the PR only and do not edit the issue.

P0/P1 findings block apply and cannot appear on a PASS report. Lower findings may appear in the summary prose with a named owner, severity, and concise context.

## CiResult

`githubctl ci-result --check|--apply --pr <n>` presents the live pull-request check-runs list as integration evidence. It GETs check-runs through structured `gh api --include` JSON. The public `CiResult` identity is `{pr, ok, jobs: [{name, conclusion, skipped}], missing, unexpected_skips}` in deterministic job-name order. It may read a head SHA internally to query check-runs and must not store, return, or bind that SHA as authority. Checks are the current PR list only, never a stored receipt.

Missing required jobs or an unexpected skip of a required job yield `ok: false`. The required set is injected (`--required`) and includes `Tama Roadmap` when tama paths change (`--tama-changed`). A skipped job that is not required is expected. Failed or incomplete conclusions fail `ok`.

`githubctl finalize-ledger --node <ID> --message <title> --date <ISO> --pr <n>` updates an existing `[[implemented]]` row's `commit_message`, timezone-bearing `commit_date`, and `pull_request` only. It never inserts a row. A missing row aborts.

`githubctl squash-land --check|--apply --pr <n> --node <ID>` squash-merges through `PUT /repos/{owner}/{repo}/pulls/{n}/merge` with `merge_method: squash`. Check plans. Apply is doctor-gated for `pullRequests` (Project 3 is not required). Apply aborts when `CiResult` is not ok (P0/P1 would be needed for failed CI). There is no post-merge ledger write, no landing receipt, and no candidate or landed SHA invariant. Closing keywords are not a merge-correctness signal.

## GitHubIssueSync

After the GH train lands, `githubctl sync-issues` owns the occasional explicit one-way synchronization run from local DAG/charter authority to GitHub. It accepts a named train or node set. In check mode it reports missing mappings and `sync_to_github = true` issues whose ordinary title or work description no longer matches the selected block after a rescope or content change. That check report is the named `IssueCreateOrUpdatePlan` (`missing`, `drift`, `protected`, `current`); apply executes the same plan and does not invent a second planner. In apply mode it creates missing issues, writes their returned mappings with `sync_to_github = true`, and updates the title/body of already mapped opt-in issues in place. Before every opt-in apply update it GETs the mapped issue through the same unambiguous read used in check (`parseIssuePayload`, including rejection of PR-shaped `pull_request != null`) and aborts `UnstructuredGitHubOutputError` before PATCH—including a GET 404. A create records `{node_id, gh_issue, kind, mapping_written: false}` as soon as GitHub returns a number; writing the local mapping flips `mapping_written`. Any later failure is `PartialFailureError` including that identity row even when it is the first operation. CLI identity is `node_id` / number / `mapping_written`, never title or body. A `sync_to_github = false` row is lookup-only: check/apply reports it as protected and never rewrites that issue. The issue number, comments, and discussion history remain intact. An update replaces ordinary work prose only; it never imports GitHub title/body/labels/state into the DAG or ledger, adds DAG metadata, or infers identity from prose. This command is run for initial issue creation, later train/node additions, or an explicit rescope refresh—not as continuous reconciliation.

## Trust and ownership

Agents are trusted to use the correct issue and PR mappings. Tooling may check local structural uniqueness, verify that a created PR body contains exactly the mapped closing link, and compare a selected mapped issue with the locally rendered description, but it never treats GitHub as authority or imports GitHub changes. Given an issue number, reverse lookup is only a search in the local `gh_issue` table; it is not reverse synchronization. GitHub closure, labels, milestones, checks, and Project fields never satisfy DAG ancestors or mark implementation complete.

P0/P1 findings block under the owning review policy. Lower-severity findings follow that policy and may be coordinated in ordinary issue or PR prose. Surviving follow-up uses `FindingCarryForward`; issue closure is not resolution. Do not build a second receipt, lifecycle, managed-body, marker, or continuous bidirectional synchronization system.

Turning an existing GitHub issue into planned DAG work is `ManualDagAuthoring`. GitHub fields never generate that authority.

## GitHubAdapter

All GitHub network effects go through `GitHubAdapter`. `programctl` remains local. The live adapter calls `gh api --include` and reads HTTP status from the structured header block (`HTTP/\d… <code>`), split from the JSON body on the first blank line (CRLF or LF). It never scrapes issue or pull-request URLs, stderr, terminal prose, or an optional JSON `status` field, and it does not classify from process exit status. Every GraphQL call, lookup or mutation, parses through one result parser: a non-object payload is unstructured; a non-empty `errors` array or missing `data` is a typed abort. Mutation methods require `mode: "check" | "apply"`. Check plans without writing. Apply requires a `GitHubDoctor` clearance minted for that adapter instance whose `owner`/`repo` match the adapter's bound repository. Owner and repo are set once at construction and are not rebindable. The network transport is constructor-injected and is not a public request port. Returned issue and pull-request numbers are JSON numbers for the caller to persist; this adapter does not write `[[github_issue]]` rows.

Issue update is a local-policy operation: a mapping with `sync_to_github === false` is refused without contacting GitHub. Opt-in updates replace title and body in place and preserve the issue number and comment history.

Pull-request creation requires the exact mapped closing link `Closes #<n>` in the body and returns the created number. `pullsForHead` lists open pull requests for a head through `GET /repos/{owner}/{repo}/pulls?head={owner}:{head}` on the same `gh api --include` JSON+HTTP-status path, parsing each list entry's `number` and `head` (`head.ref` or a string). `getPullRequest` reads one pull request through `GET /repos/{owner}/{repo}/pulls/{n}`. `createPullRequestComment` posts an ordinary PR comment through `POST /repos/{owner}/{repo}/issues/{n}/comments` and returns the pull-request `number`, never a comment URL. `listPullRequestCheckRuns` GETs the live PR check-runs list, reading a head SHA only as a query key and never returning it. `mergePullRequest` PUTs `/repos/{owner}/{repo}/pulls/{n}/merge` with `merge_method: squash` and returns `kind`, `number`, `merge_method`, and `applied` without a SHA. `getIssueLabels` reads `GET /repos/{owner}/{repo}/issues/{n}/labels` as a JSON array of `{name}` objects. `setAiResultLabel` replaces exactly one `AiOwnedLabels` result: it POSTs that one label and DELETEs the previous AI-result label when present. It never PUTs the whole label set, never creates `dag:*` labels, and never removes `ai:ignore`. Check plans locally; apply is doctor-gated for `issues`. Issue create, issue update, PR create, and PR comment records carry `kind`, the returned `number` when applied, and `applied`.

`listMilestoneIssues(title)` GETs `/repos/{owner}/{repo}/milestones?state=all`, selects the matching title, then GETs `/repos/{owner}/{repo}/issues?milestone={n}&state=all`. It skips `pull_request != null` rows and returns `{number, title, state, milestone}`. A missing title is `NotFoundError`.

`dispatchReleaseRehearsal({ mode, clearance, ref? })` POSTs `/repos/{owner}/{repo}/actions/workflows/release-check.yml/dispatches` with `{ ref }` (default `main`). Check plans without writing. Apply is doctor-gated for `actions`; mode is required and is never defaulted to apply. HTTP 204 means GitHub accepted the dispatch, not that the rehearsal job passed. The planner records `terminal_result: "pending"` and does not fold `dispatched: true` into `plan.ok`. Live Actions run polling is not default: the rehearsal runs the full paid release graph.

## GitHubDoctor

`GitHubDoctor` is check-only. It validates authentication, repository access, and the mutation capabilities a command requires before a write. It never creates, updates, or labels resources and it never stores credentials. `inspectCapabilities` returns one `{authenticated, login?, repository, issues, pullRequests, projects, actions}` record. `projects` is true only when GitHub Project 3 is readable for the adapter owner. Live `actions` is the workflow_dispatch write proxy: GitHub's repository permission object has no distinct Actions bit, so it is the same push/maintain/admin write signal as `pullRequests`. `FakeGitHubAdapter` models `permissions.actions` independently. Expected misses (unauthenticated, missing repository, wrong `full_name`, missing issue write, missing pull-request write, missing Project 3, missing Actions write) do not throw; `check({ require })` folds them into `{ok, errors, capabilities, clearance}`. The `doctor` CLI requires issues, pull-requests, and Project 3 and does not require `actions`. `sync-issues --apply` requires `issues` and must not fail solely because Project 3 is unreadable. `inspect --apply` requires `issues` and must not fail solely because Project 3 is unreadable. `create-pr --apply` and `review-summary --apply` require `issues` and `pullRequests` and must not fail solely because Project 3 is unreadable. `squash-land --apply` requires `pullRequests` and must not fail solely because Project 3 is unreadable. `schedule --apply` requires `issues` and `projects`. `release-plan --dispatch` requires `actions` and must not fail solely because Project 3 is unreadable. Unauthenticated and missing-repository folds come from HTTP 401/403/404, not from JSON `status`. GraphQL HTTP 200 with `errors` or a null Project 3 is a missing Project identity, not a title to guess. Unstructured output (missing HTTP status line or non-object JSON) is the only inspect failure that throws. Apply clearance is minted only by `check()` for that adapter instance; a hand-built `{kind, owner, repo, issues:true}` object is not clearance, and a minted clearance for a different owner/repo is not clearance.

## FakeGitHubAdapter

`FakeGitHubAdapter` implements the same methods as `GitHubAdapter` for tests. It assigns deterministic issue and pull-request numbers from one shared monotonic sequence, preserves comment lists across opt-in updates, records pull-request comments, records protected-mapping refusals without mutation, requires the exact `Closes #<n>` link, records squash merges without SHA identity, models issue labels, records AI-result label writes without whole-set replacement, and reports partial failure by the numbers already returned. Issue bodies remain ordinary text; tests count `Model:` lines on those bodies. Check mode plans locally without existence lookup; apply owns 404 and duplicate. Project 3 is present by default; `{ projectNumber: 3, missing: true }` makes it missing. Adding an issue to Project 3 is idempotent. Apply `addIssueToProject` 404s a missing issue, matching live. The fake refuses to create a project other than Project 3 and writes a milestone only when `setIssueMilestone` is called. Apply `dispatchReleaseRehearsal` requires minted `actions` clearance and never defaults mode to apply. Live GitHub is not a test substrate.

## ReadySchedulingPlan

`githubctl schedule --check|--apply` is the maintainer-owned scheduling overlay. READY comes only from `deriveState` / `programctl` (loadAuthority in-process). GitHub Project Status, labels, and milestones never change DAG readiness. Check plans without adding project items or writing milestones. Apply is doctor-gated. For each selected node that is READY and has a local `[[github_issue]]` mapping, apply adds that issue to GitHub Project 3, the one long-lived project, idempotently if it is already a member. Explicit `--nodes` aborts when any selected node is not READY, including COMPLETE and BLOCKED. `--train` keeps only READY nodes from that train, in deterministic topological order among READY, and aborts when that set is empty. A selected READY node without a local mapping aborts. Missing Project 3 aborts. This command does not attach project items from `sync-issues`.

The frozen Project 3 view names, recorded on the plan and not used as authority, are `execution`, `READY`, `triage`, `review/gate`, `train`, `milestone`, and `roadmap`. Do not create per-release projects. Do not treat Project Status as the READY frontier.

`GitHubAdapter.addIssueToProject({ number: 3, issueNumber, mode, clearance })` is the project-membership mutation. Projects v2 traffic uses structured `gh api --include graphql` JSON, with HTTP status from the header block; it does not scrape `gh project` prose. Every GraphQL call, including mutations, parses through one result parser: a non-object payload is unstructured; a non-empty `errors` array or missing `data` is a typed abort (`MissingProjectIdentityError` for Project identity, `NotFoundError` or `GitHubAdapterError` otherwise). `addIssueToProject` returns `applied: true` only when the mutation payload includes `item.id`, or Project 3 membership is already proven. `already_member` is true when the issue is already in the project, false when the membership list proves it is not, and omitted when membership cannot be known. `setIssueMilestone` returns `applied: true` only when the mutation payload confirms the issue number.

## MilestoneOverlay

A milestone is the intended or earliest release, owned by maintainers. The scheduling plan may read an issue milestone as overlay metadata. A milestone must never make a BLOCKED node READY, erase a finding carry-forward obligation, or change P0/P1 status. `deriveState` ignores GitHub.

## ReleaseTarget

`--set-milestone <title>` is the only milestone write instruction. Without that flag, schedule must not PATCH a milestone. With the flag, apply sets the release target on the mapped **issue**, never on a PR. The adapter never moves a milestone unless that instruction is present.

## ReleaseReadiness

`githubctl release-plan --check|--apply --milestone <title>` is the maintainer-owned milestone release planner. Readiness comes only from local `[[implemented]]` rows. GitHub issue closure, labels, Project Status, and milestone progress never complete a node.

Inspect milestone issues through structured adapter JSON (`listMilestoneIssues`). Map each item to the DAG through the local `[[github_issue]]` table only. A mapped item is `ReleaseReadiness` iff its node has an `[[implemented]]` row. Check and apply compute the same plan. Apply does not write GitHub. It records rehearsal identity `{workflow: "release-check.yml", uses: "release.yml", dry_run: true}` from the job in `.github/workflows/release-check.yml` that `uses: ./.github/workflows/release.yml` and whose `with.dry_run` is YAML-true; a comment mentioning `dry_run: true` is not identity. Live `workflow_dispatch` requires explicit `--dispatch`, is doctor-gated for `actions`, and is never the default. A 204 dispatch records `terminal_result: "pending"` and does not make `plan.ok` true; `plan.ok` is ledger-blocker emptiness. Live job poll is not default. Do not create a duplicate release validator.

## ReleaseBlocker

`ReleaseBlocker` is the deterministic report of every item that prevents release readiness: unmapped milestone issues, mapped nodes without an `[[implemented]]` row, missing predecessor ledger rows (every `deriveState` ancestor, not only direct predecessors), and P0/P1 `FindingCarryForward` records. Silent waiver is forbidden. Maintainer waiver of an unmapped item requires `--waive-item <n>` and cannot be inferred from GitHub state. Waiving a mapped DAG item is refused as ambiguous. Mutable GitHub closure cannot erase a carry-forward obligation.

## AiIssueVerdict

`AiIssueVerdict` is the closed, mutually exclusive AI-result state of a non-DAG issue. The only results are:

- `unchecked` — no AI verdict yet
- `confirmed` — the issue is a valid product problem against current source
- `rejected` — the issue is not a valid product problem
- `fixed` — current source already addresses the issue
- `needs-human` — evidence is insufficient or a product decision is required

These five names are the only AI results. `ai:checked` is rejected: it has no distinct semantics from this set and must not be created, reused, or treated as a verdict. An issue carries at most one AI-result label. Applying a new verdict replaces only the previous AI-result label.

`AiIssueVerdict` never encodes DAG identity, topology, readiness, or implementation-ledger state. A verdict does not promote work into the DAG.

## AiOwnedLabels

`AiOwnedLabels` is the closed AI-owned GitHub label namespace. The only AI-owned labels are the `AiIssueVerdict` spellings:

- `ai:unchecked`
- `ai:confirmed`
- `ai:rejected`
- `ai:fixed`
- `ai:needs-human`

Before creating an AI-result label, inspect the issue's current labels and reuse this vocabulary rather than a duplicate namespace. AI may add, replace, or remove only labels in this set, and only one at a time. Unrelated labels and maintainer-owned labels are preserved. Whole-label-set replacement is forbidden.

Structural or lifecycle `dag:*` labels are forbidden. GitHub must not project DAG identity, topology, readiness, or implementation-ledger rows through labels or other issue fields.

## MaintainerGuards

`MaintainerGuards` are maintainer-owned issue labels. The only maintainer-owned feedback guard is `ai:ignore`.

AI cannot create, remove, or override `ai:ignore`. Presence of `ai:ignore` is a complete no-op for AI inspection and report generation: zero GitHub mutation and zero local FeedbackReport write. Promotion of an issue into DAG work is an explicit maintainer action in an ordinary reviewed patch and is never inferred from `ai:ignore`, from any `AiIssueVerdict`, or from any other label.

## FeedbackReport

`FeedbackReport` is local operational evidence for a non-DAG issue inspection. `githubctl inspect --check|--apply --issue <n> --verdict <AiIssueVerdict>` writes `.feedback/issues/<issue-number>.md` (overridable with `--report-dir`). The report is not static DAG authority, is not committed by default, and is not a GitHub-side projection.

A report records issue identity, timezone-bearing inspection date, classification, reproduction, code paths, commands, `AiIssueVerdict`, confidence or ambiguity, owner hint, and recommendation. It does not record DAG identity, topology, readiness, or implementation-ledger rows. Inspection reads current GitHub identity and labels and records the caller verdict; it does not treat issue prose as authority.

Check plans without writing the report or labels. Apply writes the local report except when `ai:ignore` is present. Before any GitHub mutation, inspect looks up `[[github_issue]]`. `sync_to_github = false` may read and write the local report but must not change any GitHub field. Unmapped issues and `sync_to_github = true` replace exactly one AI-owned result label. Presence of `ai:ignore` is a complete no-op: zero report write and zero label mutation. Inspect never closes, reopens, comments, rewrites title/body, moves a milestone, writes `[[implemented]]`, or creates DAG authority. P0/P1 are not resolved by inspect.

## FindingCarryForward

`FindingCarryForward` is the durable follow-up record for a surviving review finding. Identity is `{issue, severity, owner}`:

- `issue` — a durable issue URL (`https://…`) or database id (positive decimal)
- `severity` — `P0`, `P1`, `P2`, or `P3`
- `owner` — the named finding owner

The machine schema is `roadmap/0.1.0-tama/schemas/finding-carry-forward.schema.json`. Additional properties are rejected, including DAG fields and GitHub closure.

Issue closure is not finding resolution. Closing, editing, or labeling the follow-up issue does not dispose the finding. P0 and P1 remain blocking under the owning review policy until that policy records resolution. Lower-severity findings follow the owning review policy and may use a `FindingCarryForward` issue when that policy calls for follow-up.

GitHub labels, milestones, Project Status, and implementation-ledger rows never satisfy or erase a carry-forward obligation.

## ManualDagAuthoring

`ManualDagAuthoring` is the sole path that turns an existing GitHub issue into planned DAG work. A maintainer authors one ordinary reviewed patch containing the train, node, predecessors, charter, and a `[[github_issue]]` row that reuses the original issue number with `sync_to_github = false`. Mapping presence does not mark the node implemented.

No `githubctl` or `programctl` command proposes, generates, imports, or applies DAG, charter, or ledger authority from GitHub. There is no `import-dag` command. `githubctl sync-issues` never creates or edits that local authority from GitHub and never updates a protected pre-existing issue.

The mapped issue keeps its number, comments, discussion, milestone, and unrelated labels. The patch adds no DAG metadata, managed region, parent edge, blocker edge, or `dag:*` label. Ambiguous or conflicting mappings abort: a duplicate `gh_issue` or a second mapping for the same node is refused. Issue closure cannot disposition P0/P1 or change implementation-ledger state.
