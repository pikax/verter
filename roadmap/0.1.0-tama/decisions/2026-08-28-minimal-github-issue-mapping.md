# Local issue mapping replaces DAG metadata in GitHub

- Status: accepted
- Date: 2026-08-28
- Supersedes: the GH train's machine markers, managed issue regions, projected DAG metadata, effort fields, and continuous reconciliation design
- Scope: GH0–GH6 issue creation, PR flow, and DAG-to-GitHub lookup

## Context

The original GH plan projected DAG structure into GitHub issues using hidden markers, managed body regions, generated topology and labels, ownership rules, and continuous reconciliation. That recreated the same maintenance problem removed from the implementation lifecycle: two representations had to remain synchronized, and GitHub prose became a machine protocol.

The useful requirement is smaller. Maintainers and agents need to find the GitHub issue for a DAG node and the DAG node for an issue. A direct local issue number provides that mapping without putting internal metadata into GitHub. Issue and PR content can then remain readable human context.

## Decision

The local ledger may contain mappings that are separate from implementation rows:

```toml
[[github_issue]]
node_id = "D1"
gh_issue = 1234
sync_to_github = true
```

The node/issue mapping is unique in both directions. The required local `sync_to_github` policy is `true` for issues created and maintained by DAG-to-GitHub sync, and `false` for pre-existing issues manually attached to DAG work. It is structurally checked locally but never used for readiness, and the mapping never marks the node implemented.

After GH6, `githubctl sync-issues` is the explicit occasional one-way command from local authority to GitHub. It is used for initial issue creation, later additions of selected trains or nodes, deterministic label reconciliation, milestone catalog and explicit assignment projection, native direct blocked-by projection from mapped predecessor edges, and explicitly requested prose refreshes. Normal check/apply uses the checked-in catalogs and never renders or compares existing issue prose. Apply creates requested missing issues with `sync_to_github = true`, captures returned issue numbers, writes the local mapping patch for an operator to commit, creates or corrects catalog definitions, adds/removes only catalog-owned issue labels, applies a milestone only when the node has `gh_milestone`, and reconciles only dependency edges whose endpoints both opt in. `--refresh-content` is the sole existing-issue title/body refresh and requires no model argument. Rows with `sync_to_github = false` are reported as protected and skipped without issue reads or writes. Content updates preserve the issue number, comments, and discussion history and never add DAG metadata to prose or import GitHub changes into local authority. The command does not run continuously.

Opt-in GitHub issues contain useful human description rather than serialized charter sections. Before initial creation or an explicit refresh, an agent uses the current charter and source as context to author a stable entry in `catalogs/github-issue-content.toml`. The entry contains a short action/outcome title, `Problem`, `Expected outcome`, and three to six observable `Acceptance` statements; optional technical context is limited to three concrete API/surface bullets. The first paragraph states the current defect and impact so the issue stands alone without roadmap access. Sync renders only that reviewed entry, fails closed when it is absent or invalid, and never mechanically extracts charter prose. Normal sync does not regenerate existing prose. The body ends with:

```text
AI-Generated
```

They do not contain model names, copied charter prose/headings, program/revision wording, DAG/block/train/phase/stage IDs, source-plan condition, predecessors/readiness, effort/budgets, owner-transition slogans, generic forbidden-design lists, deletion inventories without explanatory context, abort conditions, gates/review instructions, verification commands, generated lifecycle labels, hidden markers, or managed metadata regions. The hard body limit is 350 words excluding the provenance footer.

When implementation begins, the agent resolves the exact local issue mapping and gives the independently landable node a dedicated worktree and branch before mutation. For an opt-in mapping that `schedule` has already added to Project 3, it sets the item to `In Progress`; a protected mapping remains maintainer-owned and receives no Project traffic. After the first implementation commit is pushed, the agent opens a draft PR with the expected final conventional-commit title and puts `Closes #<gh_issue>` in its body. The reviewed PR is the implementation landing path and is squash-merged through GitHub; it is not an after-the-fact mirror of a candidate landed locally first. GitHub therefore attaches the issue to the PR and closes it only when that PR merges. A shared multi-node worktree/branch/PR is allowed only when the user or maintainer explicitly requests one atomic landing before mutation and records why the included nodes cannot land independently; every included node retains its own mapping, ledger row, and closing link. The agent adds the useful description and final `AI-Generated` footer only when `sync_to_github = true`; a protected issue is never edited by the agent, although the PR still carries its closing link. At the end, before squash and final review, the finishing agent completes the node's `[[implemented]]` row with the planned commit message, approximate timezone-bearing date, and known PR number. After merge, an opt-in issue becomes `Done`; its native train parent follows the first child to `In Progress` and becomes `Done` only when every locally mapped train child is done. A protected parent or protected mapped child suppresses automated parent roll-up.

When the user or maintainer explicitly chooses a non-PR landing, each included node still resolves that mapping and the final squash commit body carries one `Closes #<gh_issue>` line per node before review. Reaching the origin default branch closes the mapped issues and is the earliest point at which an opt-in mapping's Project status becomes `Done`; protected mappings remain maintainer-owned. This is the only location change: mapped DAG issues remain coordination metadata and are never source or test rationale.

## Consequences

- GitHub remains a human coordination surface rather than a second DAG database.
- Node-to-issue and issue-number-to-node lookup are simple searches in one local table; the latter is lookup, not reverse synchronization.
- Issue creation requires a small mapping commit, but no metadata is injected into GitHub.
- Existing opt-in issue prose is stable during normal sync and may be regenerated only with explicit `--refresh-content`; protected mappings remain untouched.
- Label vocabulary, colors, descriptions, and train/kind classification are versioned locally, so no agent or model chooses labels during a sync run.
- Every implementation PR carries the mapped issue's ordinary GitHub closing link, so merge closes the issue without a separate reconciliation step.
- Each independently landable node uses its own worktree and branch by default, plus its own PR when GitHub control is active; a shared atomic train landing requires explicit advance approval.
- A non-PR squash carries one mapped-issue closing line per included node, so origin-default-branch landing closes the DAG issues without a PR.
- With GitHub control active, merging the reviewed PR is the landing operation rather than documentation of an earlier local landing.
- Partial external failure is reported explicitly; tooling does not guess identity from titles or bodies.
- Adding an existing issue to the DAG is always a manually authored DAG/charter/mapping patch with `sync_to_github = false`; it is never an automatic or reverse-sync operation, and the issue is never rewritten by sync.
- The old transferred GH source remains historical evidence and is superseded wherever it requires projection, markers, effort fields, managed regions, identity binding, or continuous reconciliation.
