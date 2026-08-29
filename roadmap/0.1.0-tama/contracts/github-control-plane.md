# Minimal GitHub workflow contract

GH0 may begin when every transitive ancestor has an implemented-ledger row. Every later GH, FB, and REL node uses the same rule.

The static DAG and local ledger remain authority. GitHub is a human coordination surface. Issues and PRs do not carry or validate serialized DAG state. Native GitHub blocked-by, parent/sub-issue, milestone, and Project Status fields are projections for human control only.

The GH and REL implementation charters are landed historical acceptance records and remain unchanged. This contract owns subsequent GitHub operating-policy amendments and supersedes older operational wording in those landed charters.

## MinimalGitHubWorkflow

`githubctl check` prints the frozen composed-workflow inventory (`kind: MinimalGitHubWorkflow`). It is local JSON and never contacts GitHub. The inventory lists the landed owners in order: `sync-issues`, `project-status`, `create-pr`, `review-summary`, `ci-result`, `finalize-ledger`, `squash-land`, `inspect`, `schedule`, `release-plan`, and `release-cut`. `sync_issues_available` remains true: `githubctl sync-issues` stays the explicit one-way mapping command after cutover. The inventory does not create a second planner, a second release pipeline, or a runner that replaces those owners.

## GitHubIssueMapping

The ledger may contain mappings that are separate from implementation rows:

```toml
[[github_issue]]
node_id = "D1"
gh_issue = 1234
sync_to_github = true
```

`GitHubIssueMapping` identity is exactly `{node_id, gh_issue}`, unique both ways. Titles, labels, GitHub state, and other issue prose are not identity. The required `sync_to_github` boolean is a local mutation policy on that identity, never a uniqueness key and never readiness: `true` permits the one-way DAG/charter-to-GitHub refresh, while `false` protects a pre-existing issue that was manually mapped into the DAG. A GitHub issue does not need a DAG marker, hidden comment, managed region, serialized predecessor list, effort tier, state label, or other generated metadata. Native GitHub blocked-by edges may mirror direct mapped predecessors without putting topology in prose. From a node, read `gh_issue`; from an issue number, search the same table with `programctl github-issue`. `programctl github-issues` lists the three stored fields (`node_id`, `gh_issue`, `sync_to_github`) sorted by `node_id`. A `[[github_issue]]` row never marks the node implemented.

## GitHubIssueDescription

An opt-in issue is a human explanation of a product or engineering problem, not a projection of the charter. The agent reads the charter and current source as context, then authors a stable ordinary-language catalog entry for review. The rendered issue must be understandable without access to the roadmap.

The title is a short action or outcome phrase. Do not blindly copy a node name, prefix it with a node ID, or mention the program, revision, DAG, block, train, phase, or stage.

The body uses this order:

```markdown
## Problem

Two to four sentences explaining what is wrong today, where it occurs, and the concrete correctness, user, or maintainer impact.

## Expected outcome

One short paragraph describing the durable behavior and authority after the work lands.

## Acceptance

- Three to six observable, independently checkable results.

AI-Generated
```

An optional `## Technical context` of at most three bullets may appear between outcome and acceptance when concrete APIs or source surfaces materially help a human reader. Target 120–250 words and never exceed 350 words excluding the final provenance footer.

Do not copy charter headings or prose. Omit independently acceptable outcome boilerplate, source-plan/program conditions, revision and repository-basis text, predecessors/readiness, owner-transition slogans, effort/budgets, dispatch or rescope mechanics, generic forbidden-design lists, deletion inventories without explanatory context, abort conditions, gate/review instructions, verification commands, implementation sequence, generated labels, markers, and metadata blocks. A constraint belongs only when it explains the problem or is an observable acceptance result. The first paragraph must identify the actual defect and its impact; acceptance bullets must stand on their own. Stable, reviewed issue fields live in `catalogs/github-issue-content.toml`, keyed locally by node ID; neither check nor apply derives prose mechanically from a charter. Before initial creation or an explicit refresh, an agent authors or updates that catalog entry once using the charter and current source as context. Missing content aborts before GitHub mutation.

The catalog renderer ends the body with exactly one provenance footer. Model names are never stored on the issue:

```text
AI-Generated
```

Check mode rejects missing catalog content, a missing required section, prohibited program prose, a body over the limit, an invalid acceptance count, or a first paragraph that does not state a concrete problem and impact. An explicit rescope sync updates the opt-in issue's title and complete body from the reviewed catalog entry while retaining exactly one final `AI-Generated` footer. Normal sync never re-renders existing prose. A protected issue is left byte-for-byte untouched by this workflow. Titles and prose are not identity; the local `gh_issue` mapping is.

## ExpectedPullRequestTitle

Before mutation, the agent resolves the exact local issue mapping and creates the independently landable node's dedicated worktree and branch. For `sync_to_github = true`, it schedules the issue and runs `githubctl project-status --apply --node <ID> --status in-progress`, including for work that will remain local until landing. A protected mapping receives no Project command. After the first implementation commit is pushed, the agent opens a draft PR with the expected final conventional-commit title. Its body contains GitHub's ordinary closing link, `Closes #<gh_issue>`, using the exact local mapping. This attaches the issue to the PR and lets GitHub close it only when that PR merges; an abandoned or closed-without-merge PR leaves the issue open. The closing link is required for both mapping policies. For `sync_to_github = true`, the agent preserves useful issue prose and its `AI-Generated` footer. For `false`, the agent does not edit the issue body or title because the project did not author it. The PR may carry normal review and validation prose, but no serialized DAG metadata or effort block is required.

One node/worktree/branch/PR is the default. Multiple nodes may share them only when the user or maintainer explicitly requests one atomic landing before mutation and records why the nodes cannot land independently; every included node retains its own mapping, ledger row, and closing link. The reviewed PR remains the candidate and GH5 lands it by squash-merging through GitHub. Never land locally first and mirror the result afterward.

`githubctl create-pr --check|--apply --node <ID> --title <final conventional commit> --head <branch>` owns that start flow. Check plans; apply is doctor-gated for `issues` and `pullRequests` (Project 3 is not required). The PR body is ordinary prose plus exactly one `Closes #<n>` produced by `mappedClosingLink`; `Fixes`/`Close` and a second closing link are rejected. Apply never attaches Project 3. `--write-locator` sets `pull_request` on an existing `[[implemented]]` row only; it never invents a row or infers COMPLETE from PR state. If that row already locates a pull request, abort before issue update or PR create, independent of `--write-locator`. Missing mapping, a missing ancestor ledger row, an existing PR for the same head, or the wrong repository abort before PATCH/POST. One node, one PR. Check and apply list open pulls for that head through the shared adapter method `pullsForHead`.

Before squash and review finish, the completing agent adds or updates the node's `[[implemented]]` row with the planned final `commit_message`, approximate timezone-bearing `commit_date`, and known `pull_request`. The row is part of the implementation patch. The PR number remains an unvalidated locator, and no post-merge reconciliation or SHA restamping follows.

## Non-PR closing flow

When a user or maintainer explicitly directs DAG work to land without a GitHub PR, the local `[[github_issue]]` mapping remains authoritative. Before final review, the squash commit body contains one exact `Closes #<gh_issue>` line per included node. The default one-node candidate therefore has one line; an explicitly approved atomic multi-node candidate has one line for each mapping. GitHub closes those issues only when the reviewed commit reaches the origin default branch. After that commit reaches the origin default branch, run `githubctl project-status --apply --node <ID> --status done` for each opt-in landed node; protected mappings receive no Project command. This exception changes the landing carrier, not source/test policy: mapped issue citations remain forbidden in source and tests.

## ReviewCycleSummary

`githubctl review-summary --check|--apply --pr <n> --node <ID> --verdict PASS|FAIL --body <human prose>` records one human-written review cycle on the mapped pull request as an ordinary issue comment. The named `ReviewCycleSummary` covers problem, scope, validation, and review in ordinary prose. It is not a managed region and does not carry DAG metadata, effort, a commit SHA, or a digest binding.

Check plans. Apply is doctor-gated for `issues` and `pullRequests` (Project 3 is not required) and posts through structured `gh api` JSON (`POST /repos/{owner}/{repo}/issues/{n}/comments`). Apply never attaches Project 3, never closes an issue, never writes `[[implemented]]` rows, and never treats closing or editing an issue as finding resolution.

The pull request must contain the exact mapped `Closes #<n>` link or be the node's located `pull_request`. A wrong mapping aborts before comment or issue mutation.

For `sync_to_github = true`, apply ensures the issue description ends with exactly one `AI-Generated` footer, stripping old model-attribution lines, duplicate provenance footers, and effort fields without replacing human prose. Protected mappings (`sync_to_github = false`) record the review on the PR only and do not edit the issue.

P0/P1 findings block apply and cannot appear on a PASS report. Lower findings may appear in the summary prose with a named owner, severity, and concise context.

## CiResult

`githubctl ci-result --check|--apply --pr <n>` presents the live pull-request check-runs list as integration evidence. It GETs check-runs through structured `gh api --include` JSON. The public `CiResult` identity is `{pr, ok, jobs: [{name, conclusion, skipped}], missing, unexpected_skips}` in deterministic job-name order. It may read a head SHA internally to query check-runs and must not store, return, or bind that SHA as authority. Checks are the current PR list only, never a stored receipt.

Missing required jobs or an unexpected skip of a required job yield `ok: false`. The required set is injected (`--required`) and includes `Tama Roadmap` when tama paths change (`--tama-changed`). A skipped job that is not required is expected. Failed or incomplete conclusions fail `ok`.

`githubctl finalize-ledger --node <ID> --message <title> --date <ISO> --pr <n>` updates an existing `[[implemented]]` row's `commit_message`, timezone-bearing `commit_date`, and `pull_request` only. It never inserts a row. A missing row aborts.

`githubctl squash-land --check|--apply --pr <n> --node <ID>` squash-merges through `PUT /repos/{owner}/{repo}/pulls/{n}/merge` with `merge_method: squash`. Check plans. For an opt-in mapped issue, apply is doctor-gated for `pullRequests` and Project 3. It marks the child Done only after merge succeeds and rolls its native parent Done only when every locally mapped child in that train is Done; otherwise the parent remains In Progress. A post-merge status failure is a `PartialFailureError` naming the successful merge and is repaired with `project-status`, never by merging again. Apply aborts when `CiResult` is not ok (P0/P1 would be needed for failed CI). There is no post-merge ledger write, no landing receipt, and no candidate or landed SHA invariant. Closing keywords are not a merge-correctness signal.

## GitHubIssueSync

After the GH train lands, `githubctl sync-issues` owns occasional explicit one-way synchronization from local authority to GitHub for a named train or node set. A normal run reconciles the versioned repository label and milestone catalogs, managed labels, explicit `gh_milestone` assignments, and native blocked-by relationships on selected opt-in issues. It does not render, compare, or PATCH an existing issue's title or body. `--refresh-content` is the sole existing-issue prose refresh; it renders the human issue description with the stable `AI-Generated` footer, compares it with the mapped issue, and PATCHes only when different. Neither refresh nor missing-issue creation requires or stores a model name. Closed GitHub issues are reported and receive no issue, label, milestone, or dependency mutation. Local completion prevents automatic blocker expansion into an already-landed node; an explicit selection may still synchronize that node's open GitHub issue.

The check report remains the named `IssueCreateOrUpdatePlan` (`missing`, `drift`, `protected`, `closed`, `current`) and adds `label_catalog` and `milestone_catalog` status plus per-issue `milestone`, `add_blocked_by`, `remove_blocked_by`, and unresolved/protected predecessor fields. Label, relationship, milestone, and, only under `--refresh-content`, prose drift place an opt-in mapping in `drift`; apply executes that plan. A selection with unresolved predecessor nodes outside its boundary returns `ok: false`, lists `required_blocker_issues`, and performs no mutation. `--create-blockers` explicitly expands the selection through the unresolved predecessor closure, creates missing issues in topological order, and writes their mappings before its relationship pass. `--ignore-blockers` explicitly keeps the original boundary and preserves omitted relationships without adding or removing them. The two flags are mutually exclusive. Before an opt-in existing-issue operation the command GETs the mapped issue through the unambiguous issue read (`parseIssuePayload`, including rejection of PR-shaped `pull_request != null`) and aborts `UnstructuredGitHubOutputError` before mutation—including a GET 404. A create records `{node_id, gh_issue, kind, mapping_written: false}` as soon as GitHub returns a number; writing the local mapping flips `mapping_written`. Any later failure is `PartialFailureError` including that identity row even when it is the first operation. CLI identity is `node_id` / number / `mapping_written`, never title or body.

A direct unresolved local edge `P -> N` becomes “issue N is blocked by issue P” only when both endpoints are selected, mapped, opt-in, and open. Sync never projects a transitive edge. Selection expansion under `--create-blockers` may recursively include direct predecessors so each resulting relationship remains direct, but it never includes a locally complete predecessor automatically. A protected predecessor is reported and its relationship is preserved. `--ignore-blockers` preserves every relationship crossing the explicit selection boundary. Stale dependencies are removed only when both endpoints remain selected, open, mapped, and opt-in; completed, closed, ignored, unmapped/manual, and protected relationships are not invented as active blockers.

`roadmap/0.1.0-tama/catalogs/github-milestones.toml` owns milestone titles and descriptions; the exact title is the stable catalog identity. The optional node field `gh_milestone` owns assignment. Absence means unmanaged/preserve, allowing a gradual maintainer sweep; it never means clear. Sync creates a missing exact-title milestone and corrects the description of an existing exact-title milestone without deleting, renaming, opening, closing, or changing due dates. It assigns a selected issue only when its block has `gh_milestone`.

A `sync_to_github = false` row is lookup-only: check/apply reports it as protected and never reads or mutates that issue. Issue numbers, comments, and discussion history remain intact. Sync never imports GitHub title/body/labels/state into local authority, adds DAG prose, or infers identity from prose. It is run for initial issue creation, catalog/relationship reconciliation, later train/node additions, or an explicit prose refresh—not as continuous reconciliation.

## IssueSyncLabels

`roadmap/0.1.0-tama/catalogs/github-issue-labels.toml` is the sole vocabulary, presentation, and classification authority for issue-sync labels. Classification is a deterministic lookup over existing train and kind fields; it never reads issue prose, invokes a model, or lets an agent invent labels during sync. Every selected opt-in issue receives exactly one `area:*`, exactly one `problem:*`, zero or one `framework:*`, and `origin:ai`. The provenance label means the initial title and description were generated by AI; later maintainer edits or explicit refreshes do not change that historical fact.

Normal check/apply reads the repository catalog. Apply creates missing catalog definitions and corrects catalog-owned names, colors, and descriptions. Repository label definitions not named by the current catalog are preserved. For each selected opt-in issue, sync adds missing desired labels and removes stale assignments only from the catalog's managed prefixes and exact-name set. It uses additive POST and single-label DELETE operations, never whole-set replacement. Unrelated labels, maintainer guards, and all AI-inspection result labels are preserved. A protected mapping receives no label read or write. Structural or lifecycle `dag:*` labels remain forbidden.

## Trust and ownership

Agents are trusted to use the correct issue and PR mappings. Tooling may check local structural uniqueness, verify that a created PR body contains exactly the mapped closing link, and, only during an explicit content refresh, compare a selected mapped issue with the locally rendered description. It never treats GitHub as authority or imports GitHub changes. Given an issue number, reverse lookup is only a search in the local `gh_issue` table; it is not reverse synchronization. GitHub closure, labels, milestones, checks, and Project fields never satisfy DAG ancestors or mark implementation complete.

P0/P1 findings block under the owning review policy. Lower-severity findings follow that policy and may be coordinated in ordinary issue or PR prose. Surviving follow-up uses `FindingCarryForward`; issue closure is not resolution. Do not build a second receipt, lifecycle, managed-body, marker, or continuous bidirectional synchronization system.

Turning an existing GitHub issue into planned DAG work is `ManualDagAuthoring`. GitHub fields never generate that authority.

## GitHubAdapter

All GitHub network effects go through `GitHubAdapter`. `programctl` remains local. The live adapter calls `gh api --include` and reads HTTP status from the structured header block (`HTTP/\d… <code>`), split from the JSON body on the first blank line (CRLF or LF). It never scrapes issue or pull-request URLs, stderr, terminal prose, or an optional JSON `status` field, and it does not classify from process exit status. Every GraphQL call, lookup or mutation, parses through one result parser: a non-object payload is unstructured; a non-empty `errors` array or missing `data` is a typed abort. Mutation methods require `mode: "check" | "apply"`. Check plans without writing. Apply requires a `GitHubDoctor` clearance minted for that adapter instance whose `owner`/`repo` match the adapter's bound repository. Owner and repo are set once at construction and are not rebindable. The network transport is constructor-injected and is not a public request port. Returned issue and pull-request numbers are JSON numbers for the caller to persist; this adapter does not write `[[github_issue]]` rows.

Issue update is a local-policy operation: a mapping with `sync_to_github === false` is refused without contacting GitHub. Explicit opt-in content updates replace title and body in place and preserve the issue number and comment history. Managed label mutations enforce the same mapping policy.

Pull-request creation requires the exact mapped closing link `Closes #<n>` in the body and returns the created number. Release pull requests use `createReleasePullRequest` instead: the title must match `/^release: v.+$/` with no `(#n)` suffix, the body must not contain a GitHub closing reference (keyword, optional colon, `#n`, `owner/repo#n`, or issue URL), and `mappedIssue` is refused. GH3 still requires the exact mapped `Closes #<n>` string; colon, `owner/repo#n`, and URL forms do not satisfy it. `mergePullRequest` may pass `commit_title` only when it is that same release subject, so squash landing does not append a PR suffix. `closeMilestone` PATCHes a milestone to `state: closed` and is never implied by pull-request creation. `pullsForHead` lists open pull requests for a head through `GET /repos/{owner}/{repo}/pulls?head={owner}:{head}` on the same `gh api --include` JSON+HTTP-status path, parsing each list entry's `number` and `head` (`head.ref` or a string). `getPullRequest` reads one pull request through `GET /repos/{owner}/{repo}/pulls/{n}`. `createPullRequestComment` posts an ordinary PR comment through `POST /repos/{owner}/{repo}/issues/{n}/comments` and returns the pull-request `number`, never a comment URL. `listPullRequestCheckRuns` GETs the live PR check-runs list, reading a head SHA only as a query key and never returning it. `mergePullRequest` PUTs `/repos/{owner}/{repo}/pulls/{n}/merge` with `merge_method: squash` and returns `kind`, `number`, `merge_method`, and `applied` without a SHA. `getIssueLabels` reads `GET /repos/{owner}/{repo}/issues/{n}/labels` as a JSON array of `{name}` objects. Repository label reads use `GET /repos/{owner}/{repo}/labels`; catalog definition creation and correction use POST and PATCH on that collection. Managed issue labels use additive POST and one-label DELETE endpoints. `setAiResultLabel` replaces exactly one `AiOwnedLabels` result through the same additive/remove primitives. Neither path PUTs the whole label set, creates `dag:*` labels, or removes `ai:ignore`. Check plans locally; apply is doctor-gated for `issues`. Issue create, issue update, PR create, and PR comment records carry `kind`, the returned `number` when applied, and `applied`.

Repository milestone reconciliation exhausts `/repos/{owner}/{repo}/milestones?state=all`, creates missing exact-title definitions with POST, and updates only the description of an existing exact-title definition with PATCH. The exact title is immutable catalog identity: sync does not rename a milestone. `setIssueMilestone` assigns the mapped issue and never a PR. `listMilestoneIssues(title)` selects the matching title, then exhausts `/repos/{owner}/{repo}/issues?milestone={n}&state=all`. It skips `pull_request != null` rows and returns `{number, title, state, milestone}`. A missing title is `NotFoundError`.

Issue dependencies use GitHub's native `/issues/{n}/dependencies/blocked_by` endpoints. List responses retain the owner, repository, issue number, and numeric database id, and reconciliation compares that complete identity so an unrelated repository's same-number issue is preserved. Add resolves the exact blocker database id and POSTs `{issue_id}`; remove DELETEs that blocker database id. Both endpoint mappings must be opt-in. Project lifecycle reads the native issue parent/sub-issue graph and Project 3 item statuses through GraphQL, then writes only the Status single-select field.

`dispatchReleaseRehearsal({ mode, clearance, ref? })` POSTs `/repos/{owner}/{repo}/actions/workflows/release-check.yml/dispatches` with `{ ref }` (default `main`). Check plans without writing. Apply is doctor-gated for `actions`; mode is required and is never defaulted to apply. HTTP 204 means GitHub accepted the dispatch, not that the rehearsal job passed. The planner records `terminal_result: "pending"` and does not fold `dispatched: true` into `plan.ok`. Live Actions run polling is not default: the rehearsal runs the full paid release graph.

## GitHubDoctor

Capability inspection probes Project 3 only when the caller requires `projects`; unrequested capabilities remain false instead of causing unrelated Project traffic.

`GitHubDoctor` is check-only. It validates authentication, repository access, and the mutation capabilities a command requires before a write. It never creates, updates, or labels resources and it never stores credentials. `inspectCapabilities` returns one `{authenticated, login?, repository, issues, pullRequests, projects, actions}` record. `projects` is true only when Project 3 resolves for the adapter owner and GraphQL reports `viewerCanUpdate: true`. Live `actions` is the workflow_dispatch write proxy: GitHub's repository permission object has no distinct Actions bit, so it is the same push/maintain/admin write signal as `pullRequests`. `FakeGitHubAdapter` models `permissions.actions` independently. Expected misses (unauthenticated, missing repository, wrong `full_name`, missing issue write, missing pull-request write, missing or non-writable Project 3, missing Actions write) do not throw; `check({ require })` folds them into `{ok, errors, capabilities, clearance}`. The `doctor` CLI requires issues, pull-requests, and Project 3 and does not require `actions`. `sync-issues --apply` requires `issues` and must not fail solely because Project 3 is unreadable. `project-status --apply` requires `projects`. `inspect --apply` requires `issues` and must not fail solely because Project 3 is unreadable. `create-pr --apply` and `review-summary --apply` require `issues` and `pullRequests` and must not fail solely because Project 3 is unreadable. `squash-land --apply` requires `pullRequests` and `projects` for an opt-in mapped issue; protected mappings require only `pullRequests`. `release-cut --apply` requires `pullRequests` and does not require Project 3. `schedule --apply` requires `issues` and `projects`. `release-plan --dispatch` requires `actions` and must not fail solely because Project 3 is unreadable. Unauthenticated and missing-repository folds come from HTTP 401/403/404, not from JSON `status`. GraphQL HTTP 200 with `errors` or a null Project 3 is a missing Project identity, not a title to guess. Unstructured output (missing HTTP status line or non-object JSON) is the only inspect failure that throws. Apply clearance is minted only by `check()` for that adapter instance; a hand-built `{kind, owner, repo, issues:true}` object is not clearance, and a minted clearance for a different owner/repo is not clearance.

## FakeGitHubAdapter

`FakeGitHubAdapter` implements the same methods as `GitHubAdapter` for tests. It assigns deterministic issue and pull-request numbers from one shared monotonic sequence, preserves comment lists across opt-in updates, records pull-request comments, records protected-mapping refusals without mutation, requires the exact `Closes #<n>` link, records squash merges without SHA identity, models issue labels, milestones, dependencies, parent/sub-issue relationships, and Project statuses, and reports partial failure by the numbers already returned. Issue bodies remain ordinary text; tests count the stable `AI-Generated` footer. Check mode plans locally without existence lookup; apply owns 404 and duplicate. Project 3 is present by default; `{ projectNumber: 3, missing: true }` makes it missing. Adding an issue to Project 3 is idempotent. Apply `addIssueToProject` 404s a missing issue, matching live. The fake refuses to create a project other than Project 3. Apply `dispatchReleaseRehearsal` requires minted `actions` clearance and never defaults mode to apply. `createReleasePullRequest` records `closes: null` and refuses a GitHub closing reference. Squash merges record `commit_title` only when supplied. `closeMilestone` records a close only when called. Live GitHub is not a test substrate.

## ProjectLifecycle

`githubctl project-status --check|--apply --node <ID> --status in-progress|done` is the sole reusable Project 3 lifecycle mutation. It requires an opt-in mapping whose issue and eligible native parent are already Project 3 members; `schedule` owns both memberships. Starting local implementation sets the mapped child and its native parent to In Progress. Completing a landing sets the child Done. Parent roll-up uses locally mapped nodes in the same train as the expected child set and validates that GitHub's native parent lists them; GitHub omissions or additions cannot redefine train membership. The parent becomes Done only when every expected child is Done. Parent identity is repository-qualified and fails closed if it points outside the bound repository or changes during planning. Check is non-mutating. A protected selected mapping receives zero Project reads or writes. If the parent or any locally mapped train child is protected, the opt-in child may update but automated parent roll-up is skipped so maintainer-owned state is never read or changed. Child success followed by parent failure is a `PartialFailureError` with the successful child identity.

Project lifecycle is a display projection. It never changes READY, COMPLETE, release readiness, or finding disposition.

## ReadySchedulingPlan

`githubctl schedule --check|--apply` is the maintainer-owned scheduling overlay. READY comes only from `deriveState` / `programctl` (loadAuthority in-process). GitHub Project Status, labels, and milestones never change DAG readiness. Local preflight resolves selection and mapping policy before doctor or adapter access, so a protected selection aborts with zero Project traffic. Check plans without adding project items or writing milestones. Apply is doctor-gated. For each selected node that is READY and has an opt-in local `[[github_issue]]` mapping, apply adds that issue and its eligible same-repository native parent to GitHub Project 3, the one long-lived project, idempotently if already members, and initializes each newly added item to Todo. It never resets an existing item's status. A protected parent or protected mapped child suppresses parent scheduling and later parent roll-up. Explicit `--nodes` aborts when any selected node is not READY, including COMPLETE and BLOCKED. `--train` keeps only READY nodes from that train, in deterministic topological order among READY, and aborts when that set is empty. A selected READY node without a local mapping aborts. Missing Project 3 aborts. This command does not attach project items from `sync-issues` and never writes milestones.

The frozen Project 3 view names, recorded on the plan and not used as authority, are `execution`, `READY`, `triage`, `review/gate`, `train`, `milestone`, and `roadmap`. Do not create per-release projects. Do not treat Project Status as the READY frontier.

`GitHubAdapter.addIssueToProject({ number: 3, issueNumber, mode, clearance })` is the project-membership mutation. Projects v2 traffic uses structured `gh api --include graphql` JSON, with HTTP status from the header block; it does not scrape `gh project` prose. Every GraphQL call, including mutations, parses through one result parser: a non-object payload is unstructured; a non-empty `errors` array or missing `data` is a typed abort (`MissingProjectIdentityError` for Project identity, `NotFoundError` or `GitHubAdapterError` otherwise). Membership is matched against the exact global Project ID, not merely project number. `addIssueToProject` returns `applied: true` only when the mutation payload includes `item.id`, or exact Project 3 membership is already proven. An incomplete Project field or item connection aborts rather than guessing membership. `already_member` is therefore always true or false on a successful apply. `setIssueProjectStatus` never adds a missing item. `setIssueMilestone` returns `applied: true` only when the mutation payload confirms the issue number.

## MilestoneOverlay

A milestone is the intended or earliest release, owned by maintainers. The scheduling plan may read an issue milestone as overlay metadata. A milestone must never make a BLOCKED node READY, erase a finding carry-forward obligation, or change P0/P1 status. `deriveState` ignores GitHub.

## ReleaseTarget

The optional DAG block field `gh_milestone` is the only assignment instruction. `sync-issues` owns that projection and sets the release target on the mapped **issue**, never on a PR. `schedule` has no milestone mutation flag. A block without `gh_milestone` preserves its current GitHub assignment so maintainers can sweep the DAG incrementally. Milestone closure remains explicit release policy and is never implied by sync.

## ReleaseReadiness

`githubctl release-plan --check|--apply --milestone <title>` is the maintainer-owned milestone release planner. Readiness comes only from local `[[implemented]]` rows. GitHub issue closure, labels, Project Status, and milestone progress never complete a node.

Inspect milestone issues through structured adapter JSON (`listMilestoneIssues`). Map each item to the DAG through the local `[[github_issue]]` table only. A mapped item is `ReleaseReadiness` iff its node has an `[[implemented]]` row. Check and apply compute the same plan. Apply does not write GitHub. It records rehearsal identity `{workflow: "release-check.yml", uses: "release.yml", dry_run: true}` from the job in `.github/workflows/release-check.yml` that `uses: ./.github/workflows/release.yml` and whose `with.dry_run` is YAML-true; a comment mentioning `dry_run: true` is not identity. Live `workflow_dispatch` requires explicit `--dispatch`, is doctor-gated for `actions`, and is never the default. A 204 dispatch records `terminal_result: "pending"` and does not make `plan.ok` true; `plan.ok` is ledger-blocker emptiness. Live job poll is not default. Do not create a duplicate release validator.

## ReleaseBlocker

`ReleaseBlocker` is the deterministic report of every item that prevents release readiness: unmapped milestone issues, mapped nodes without an `[[implemented]]` row, missing predecessor ledger rows (every `deriveState` ancestor, not only direct predecessors), and P0/P1 `FindingCarryForward` records. Silent waiver is forbidden. Maintainer waiver of an unmapped item requires `--waive-item <n>` and cannot be inferred from GitHub state. Waiving a mapped DAG item is refused as ambiguous. Mutable GitHub closure cannot erase a carry-forward obligation.

## ReleaseCutAuthorization

`githubctl release-cut --check|--apply --version <semver> [--authorize]` is the maintainer-owned release cut. Check may plan without `--authorize`. Apply without `--authorize` aborts and must not create a pull request, merge, or close a milestone. Apply is doctor-gated for `pullRequests` (Project 3 is not required). Authorization is not inferred from GitHub issue, milestone, or Project state.

## ReleasePullRequest

The release pull request title is exactly `release: v<version>` and must match `/^release: v.+$/` with no appended `(#n)` suffix. `create-pr` / `createPullRequest` cannot open that PR: GH3 still requires `Closes #<n>` and refuses a release subject. `createReleasePullRequest` is the sole create path, does not require a mapped issue, and must not put a GitHub closing reference in the planned or created body so the PR cannot auto-close a DAG issue. Check rejects a planned body that contains one. After explicit authorize + apply, record rehearsal identity `{workflow: "release-check.yml", uses: "release.yml", dry_run: true}` without dispatching. Reuse `.github/workflows/release-tag.yml` and `.github/workflows/release.yml`; do not add a second tag or publish workflow. The command must not auto-close the milestone; `--close-milestone` is the only close path. P0/P1 findings block apply; GitHub issue or milestone closure cannot erase them.

## ReleaseLanding

Release landing reuses squash merge after a successful `CiResult`. Apply `--land --pr <n>` is doctor-gated for `pullRequests`. Landing aborts before merge when `pull.body` contains a GitHub closing reference. The squash `commit_title` is the same `release: v<version>` subject; GitHub's default PR-number suffix is forbidden because `.github/workflows/release-tag.yml` matches `^release: v`. There is no post-merge ledger write and no landing receipt.

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

`origin:ai` is sync-owned provenance, not an inspection verdict and not part of `AiOwnedLabels`. Inspection never creates, replaces, or removes it.

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

The mapped issue keeps its number, comments, discussion, milestone, and unrelated labels. The patch adds no DAG prose, managed region, or `dag:*` label. Because its mapping is protected, sync neither adds nor removes its parent or blocker relationships; maintainers own those edges. Ambiguous or conflicting mappings abort: a duplicate `gh_issue` or a second mapping for the same node is refused. Issue closure cannot disposition P0/P1 or change implementation-ledger state.
