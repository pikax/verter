Implement a new post-ORC0 GitHub control-plane program for Rev11.

Repository:
https://github.com/pikax/verter

Rev11:
docs/arch/refactor/rev11

Existing GitHub Project:
https://github.com/users/pikax/projects/3/views/1
Owner: pikax
Project number: 3

This work MUST begin only after ORC0 has landed and its accepted receipt is current.

# Goal

Move Rev11 operational orchestration onto GitHub Issues, sub-issues, PRs, CI and milestones while preserving the architectural guarantees established by ORC0.

GitHub is to become the operational control plane and historical UI.

It MUST NOT replace Rev11's correctness authority.

The final ownership model must remain:

* static Rev11 DAG/charters/contracts = architecture authority;
* immutable `programctl` receipts = lifecycle/correctness authority;
* ephemeral leases = active work ownership;
* GitHub Issues/Projects/PRs = operational projection, coordination and history;
* GitHub CI = gate executor whose exact evidence is imported/bound into the Rev11 evidence model;
* milestones = maintainer-owned release-planning metadata.

Do not create a generic bidirectional synchronization system where either side can overwrite the other.

Implement field-owned reconciliation: every synchronized field must have exactly one authoritative owner.

# Mandatory architecture

Do not modify generated `program-dag.toml` directly.

Add proper authority DAG modules/charters/contracts using the post-ORC0 amendment mechanism.

Use these blocks unless current source proves a materially better decomposition. Do not silently combine them into larger blocks.

## governance.github-control-plane

GH0 — GitHub control-plane contract and authority matrix
Predecessor: ORC0

Define:

* authority ownership for every synchronized datum;
* GitHub structural markers;
* label namespaces;
* managed-body boundaries;
* drift classes;
* reconciliation rules;
* security/fail-closed rules;
* configuration for `pikax/verter` and Project `pikax/3`.

No live synchronization implementation yet.

GH1 — GitHub adapter, Project discovery and deterministic fixtures
Predecessor: GH0

Create a replaceable GitHub adapter layer rather than coupling GitHub directly into the core lifecycle engine.

Preferred layering:

```
programctl / trusted-local lifecycle core
            ↑
        githubctl
            ↑
      gh CLI / GitHub
```

`programctl` remains the correctness kernel.
`githubctl` is an external-control-plane adapter.

Use structured `gh` JSON/API output. Never scrape terminal prose.

At startup/doctor:

* validate `gh auth status`;
* verify repo access;
* verify Project 3 access and required `project` scope;
* inspect `gh project view 3 --owner pikax`;
* inspect `gh project field-list 3 --owner pikax`;
* inspect current labels;
* fail clearly on missing permissions instead of partially mutating state.

Build a deterministic fake/fixture GitHub adapter so tests never need to mutate live GitHub.

Every mutation API must support dry-run/check mode.

GH2 — DAG → GitHub issue projection and reconciliation
Predecessor: GH1

Project the static DAG onto GitHub:

* one train = one parent issue;
* one DAG node/block = one issue/sub-issue;
* nested DAG subblocks may become nested sub-issues;
* DAG predecessors = GitHub `blocked by` relationships;
* add issues to Project 3;
* apply structural/lifecycle labels;
* preserve all maintainer-owned metadata.

Use durable machine markers, not issue titles, to resolve identity, e.g.

```
<!-- verter:dag-train=compiler.compiler-bridge -->
<!-- verter:dag-node=CCA1 -->
```

Never use titles as identity.

Issue bodies must contain bounded managed regions. Human-authored content outside those regions must survive byte-for-byte.

Example:

```
<!-- verter:managed:start -->
...generated DAG metadata...
<!-- verter:managed:end -->

...human-maintained issue text...
```

Reconciliation must be idempotent.

A second identical `sync --apply` must produce no mutations.

Support:

```
githubctl sync --check
githubctl sync --apply
```

At minimum distinguish:

* safe repairable projection drift;
* maintainer-owned differences that must be preserved;
* dangerous/ambiguous drift that must fail closed.

Examples of dangerous drift:

* two GitHub issues claiming the same DAG node;
* one issue claiming two DAG nodes;
* a DAG node mapped to a missing repository;
* incompatible parent mappings;
* a DAG issue manually closed while no landed receipt exists;
* GitHub blocker topology that conflicts with generated DAG topology.

GitHub closure must NEVER make a DAG predecessor satisfied.

Only Rev11 receipts do that.

Do not overwrite milestones.

Milestones are maintainer-owned.

GH3 — PR-backed dispatch and draft lifecycle
Predecessor: GH2

Change block execution so a dispatched block receives/creates a GitHub draft PR early.

Desired flow:

```
DAG READY
  ↓
admit/lease
  ↓
branch
  ↓
draft PR immediately
  ↓
bind DAG node → GitHub PR
  ↓
implementation/review-fix work
  ↓
final candidate
  ↓
Ready for review
  ↓
formal gate/review
  ↓
squash landing
```

The PR number is the stable operational work identity.

Humans/agents should normally reason in:

```
CCA1 / Issue #N / PR #M
```

rather than manually exchanging candidate SHAs.

However, DO NOT weaken immutable evidence.

Internally, `programctl` must still freeze and bind:

* exact PR head SHA/tree;
* base SHA/tree;
* integration SHA/tree where applicable.

The SHA/tree is an internal immutable snapshot beneath the PR abstraction.

If PR #M changes after candidate finalization, old evidence becomes stale automatically.

Do not mutate static DAG authority merely because a PR was created.

PR bindings are runtime/external-control-plane state.

GH4 — PR body, review history and immutable evidence bridge
Predecessor: GH3

Define a managed PR-description format.

The description should be concise and useful to a human.

At minimum:

```
## Problem
short explanation of what the block solves

## Scope
what changed / important boundaries

## Validation
concise current validation state

<details>
<summary>Review cycle 1</summary>

concise findings/fixes/outcome
</details>

<details>
<summary>Review cycle 2</summary>

...
</details>

## Implementer
Harness/agent: ...
Model: ...            # when available
Effort: ...           # when available
```

The Implementer section must remain at the end of the managed PR description.

Do not dump huge raw logs into the PR description.

Raw authoritative review/gate evidence remains in the Rev11 evidence system.

The PR is the human-readable history.

Prefer updating the managed PR description over producing noisy per-cycle PR comments.

Any human content outside managed regions must be preserved.

Formal candidate finalization must bind the current PR revision.

GH5 — CI integration gate, safe squash landing and landed proof
Predecessor: GH4

Make GitHub CI the top-level integration gate executor for a block.

Do NOT replace the Rev11 evidence schema with "GitHub says green".

The required CI job must emit/import evidence bound to:

* DAG node;
* PR;
* PR head;
* exact integration base;
* exact tested integration tree;
* commands actually executed;
* unexpected skips;
* terminal result.

Use existing `programctl gate-run`/evidence primitives where possible.

The final invariant for squash merging is:

```
landed_tree == validated_integration_tree
```

NOT:

```
landed_sha == candidate_sha
```

Only the program orchestrator may squash-merge DAG block PRs.

Before merge verify:

* PR is the expected PR;
* current head equals the frozen head;
* required review evidence is current;
* required CI/gate evidence is current;
* tested integration base is still current enough under the defined policy.

If head or required integration base changed, invalidate/rerun rather than guessing.

Use expected-head protection (`--match-head-commit` or equivalent).

After squash:

* fetch the resulting main commit;
* validate landed tree against the accepted integration tree;
* emit a generic landed receipt;
* only then mark the DAG node LANDED / satisfy successors.

One DAG block should normally become one squash commit on `main`.

The detailed intermediate history belongs in its PR.

Do not use automatic `Closes #N` as correctness authority.

Use `Refs #N`.

For generated DAG issues, initially make issue auto-closing configurable and default conservatively. A `dag:landed` state must always come from verified landed evidence regardless of whether the human issue remains open.

## governance.feedback-intake

FB0 — Non-DAG issue label state machine and feedback contract
Predecessor: GH0

Design and implement a namespaced label contract.

Recommended shape:

AI result states, mutually exclusive:

```
ai:unchecked
ai:confirmed
ai:rejected
ai:fixed
ai:needs-human
```

Maintainer-controlled guards:

```
ai:ignore
dag:promotion-approved
```

DAG structure:

```
dag:train
dag:node
dag:release
```

DAG lifecycle projection:

```
dag:blocked
dag:ready
dag:leased
dag:review
dag:gate
dag:landed
```

Inspect current repository labels before creating anything.
Reuse a semantically equivalent existing label if one already exists.
Do not create duplicate vocabularies.

`ai:checked` is not required unless you find a concrete non-overlapping semantic meaning for it. Prefer `ai:needs-human` for an inspected but inconclusive issue.

`ai:ignore` is maintainer-owned. AI must never remove or override it.

`dag:promotion-approved` is maintainer-owned. AI must never create it itself.

FB1 — Non-DAG issue inspection
Predecessors: GH1, FB0

Support inspection by GitHub issue ID.

Example conceptual interface:

```
githubctl issue inspect 96
```

For a non-DAG issue:

* retrieve issue and relevant metadata;
* inspect current source;
* inspect/reproduce tests where useful;
* establish whether the problem is currently real;
* do not blindly trust old issue text;
* create/update `.feedback/issues/<issue-number>.md`;
* update only the AI-owned result labels.

The report should contain enough evidence to audit the conclusion:

* issue ID/title;
* inspected main/tree identity;
* classification;
* reproduction/verification;
* relevant code paths;
* relevant commands/tests;
* verdict;
* confidence/ambiguity;
* likely owning subsystem/train if useful;
* recommendation.

The AI must NOT:

* close the issue;
* reopen the issue;
* comment on the issue;
* rewrite the issue body/title;
* move its milestone;
* promote it to DAG work without explicit maintainer permission.

If `ai:ignore` is present, inspection is a no-op.

Suggested verdict semantics:

`ai:confirmed`
The reported problem is valid on the inspected current tree.

`ai:fixed`
The issue was meaningful, but the inspected current tree already resolves it.

`ai:rejected`
The issue's claim is contradicted or invalid under current source/contracts.

`ai:needs-human`
Inspection is complete but evidence is insufficient or a product decision is required.

Treat `.feedback/` as operational feedback evidence, not static DAG authority.
Do not mutate `main` merely to archive a triage report unless current repository policy explicitly decides that these reports are tracked.

FB2 — Maintainer-authorized issue → DAG promotion
Predecessors: FB1, GH2

A non-DAG issue may become DAG work only with explicit maintainer authorization.

The visible GitHub authorization signal may be `dag:promotion-approved`, but that mutable label alone MUST NOT be sufficient to mutate canonical DAG authority.

Use the normal post-ORC0 amendment/ratification mechanism.

Preferred process:

```
non-DAG issue
    ↓
AI inspection/report
    ↓
maintainer adds promotion authorization
    ↓
architect proposes train/node/predecessors/charter
    ↓
Rev11 amendment is ratified
    ↓
existing GitHub issue is reused as the DAG issue
    ↓
add dag:node + parent train + dependencies
    ↓
eventual implementation PR uses `Refs #N`
```

Do not duplicate the issue merely because it became DAG-managed.

The original issue discussion/body should remain intact outside the generated managed section.

The accepted DAG amendment should record the source GitHub issue as provenance.

## governance.release-control

REL0 — Milestone release-planning overlay and DAG scheduling
Predecessor: GH2

Milestones are maintainer-owned release planning.

The AI must never move an issue between milestones unless explicitly instructed by the maintainer.

Interpret an issue's milestone as its intended/earliest target release.

Examples:

```
0.1.0-beta.4
0.1.0-beta.5
0.1.0-beta.6
0.1.0-rc.1
0.1.0
```

Do not attempt to assign one issue to both beta and final. Later releases transitively contain earlier shipped work.

Use Project 3 as one long-lived project with views rather than creating a new project per release.

Useful views should include:

* DAG execution;
* READY frontier;
* non-DAG triage;
* review/gate;
* by train;
* by milestone/release;
* roadmap.

Milestones influence scheduling ONLY among already-READY DAG nodes.

Correct order:

```
programctl frontier
     ↓
READY nodes
     ↓
milestone priority
     ↓
critical-path/conflict/resource scheduling
```

Incorrect:

```
milestone says beta.4
     ↓
bypass DAG predecessor
```

Never do that.

Do not assign every block PR to the milestone if its issue is already assigned; that would double-count milestone progress. Keep the target release primarily on the work issue.

REL1 — Milestone release readiness and release rehearsal
Predecessors: REL0, GH5

Create a release planner that can inspect a milestone and determine whether a release may be cut.

Conceptual interface:

```
githubctl release plan 0.1.0-beta.5
```

For DAG-managed issues, readiness must derive from accepted/landed Rev11 evidence, not merely GitHub closure.

If a required milestone item is not ready, report exactly what blocks the release.

Do not silently waive work.
Only the maintainer can move/defer/waive release content.

Reuse the existing `.github/workflows/release-check.yml`.

It intentionally invokes the exact `release.yml` graph with `dry_run: true`.
Preserve that architecture; do not build a duplicate release validation pipeline.

REL2 — Release PR, tag and publication integration
Predecessor: REL1

After the maintainer explicitly authorizes a release cut:

```
milestone ready
     ↓
create release/version-bump branch
     ↓
draft release PR
     ↓
run exact release rehearsal
     ↓
validate
     ↓
maintainer/program-orchestrator authorizes landing
     ↓
squash merge
     ↓
exact release commit subject
     ↓
existing release-tag.yml
     ↓
existing release.yml
```

The squash commit subject must remain compatible with current release-tag.yml, currently:

```
release: v<version>
```

Example:

```
release: v0.1.0-beta.5
```

Do not append `(#PR)` if that would break the current exact matcher.

Use the existing release tagging/publishing implementation rather than replacing it.

Alpha/beta/rc releases remain prereleases according to the existing `release.yml` contract.

Do not automatically close the milestone unless the maintainer explicitly chooses that policy.

## GH6 — GitHub control-plane convergence and cutover

Predecessors:

* GH2
* GH5
* FB2
* REL2

This is the integration/convergence block, not another feature train.

Prove end-to-end:

1. static DAG projects deterministically into GitHub;
2. repeated reconciliation is a no-op;
3. train → block hierarchy is correct;
4. predecessor → blocked-by topology is correct;
5. maintainer-owned milestone/human text survives reconciliation;
6. non-DAG AI inspection is label-only on GitHub and produces the local report;
7. `ai:ignore` prevents mutation;
8. promotion without maintainer/amendment authorization fails;
9. approved promotion reuses the existing issue correctly;
10. dispatch creates/binds a draft PR;
11. PR head changes invalidate frozen evidence;
12. Ready-for-review binds an immutable candidate snapshot;
13. stale CI cannot authorize landing;
14. squash landing verifies exact validated integration content;
15. GitHub closure/labels cannot forge DAG acceptance;
16. release milestone never bypasses DAG readiness;
17. release rehearsal executes the existing exact release graph;
18. release PR integrates correctly with existing tag/publish workflows;
19. human-authored body sections are never overwritten;
20. duplicate or conflicting GitHub/DAG mappings fail closed.

Include planted RED/GREEN negative controls for the important authority boundaries.

# Synchronization ownership matrix

Implement this concept explicitly in code/contracts rather than leaving it implicit.

DAG-owned:

* node ID;
* train;
* charter-derived managed metadata;
* parent train relationship;
* DAG predecessor relationships;
* DAG lifecycle state;
* accepted/landed status.

GitHub-maintainer-owned:

* milestone;
* human issue text;
* human labels outside managed namespaces;
* `ai:ignore`;
* promotion authorization;
* manual release-cut authorization.

GitHub/runtime-owned:

* issue number;
* PR number;
* PR URL;
* draft/ready state;
* CI run/check identity;
* merge result.

AI-owned:

* AI triage result labels;
* `.feedback/issues/<id>.md`;
* managed PR summary/review-cycle presentation.

Derived/non-authoritative:

* GitHub Project Status;
* project grouping/views;
* convenience progress indicators.

Never permit derived GitHub Project state to become a correctness input.

# GitHub write discipline

Every sync/write must:

* be idempotent;
* operate by stable IDs/markers, not titles;
* use structured JSON output;
* preserve human-owned fields;
* modify labels additively/removally only inside the owned namespace;
* never replace the entire label set;
* have check/dry-run mode;
* fail closed on ambiguous identity;
* surface exact planned mutations before applying when practical;
* never persist credentials/tokens;
* tolerate reruns after partial network failure.

Do not use regex scraping of GitHub UI/prose.

Use current supported `gh issue`, `gh pr`, `gh project` and `gh api` functionality.

# PR policy

Normal DAG block:

```
one DAG block
  =
one GitHub issue
  =
one implementation PR
  =
one squash commit on main
```

Exceptions require an explicit charter reason.

Draft PR is the normal implementation state.

Only the program orchestrator lands DAG block PRs.

The block agent/implementer may:

* create/push its branch;
* create/update its draft PR;
* update managed PR description;
* execute its assigned implementation/review-fix work.

It may not independently squash-merge itself.

Use `Refs #issue`, not automatic close keywords, unless a future explicit policy changes that.

# Required tests

Do not rely on live GitHub for tests.

Create fixtures/fake adapter tests for at least:

* empty project bootstrap;
* existing correct projection;
* partially missing projection;
* duplicate DAG marker;
* human-edited body;
* human milestone movement;
* unrelated labels;
* stale/incorrect DAG lifecycle label;
* issue manually closed without landed receipt;
* non-DAG triage;
* ignored non-DAG issue;
* conflicting AI labels;
* authorized/unauthorized promotion;
* PR head update after candidate freeze;
* stale CI run;
* base movement before merge;
* squash result tree mismatch;
* interrupted sync followed by retry;
* repeated sync no-op;
* milestone release with blocked item;
* milestone release fully ready;
* beta release version/tag path.

Live GitHub smoke testing must be bounded and reversible.

Do not spam the real repository with throwaway issues. If live mutation is necessary, use a clearly named temporary fixture and clean it up only with explicit maintainer authorization.

# Existing surfaces to inspect before implementation

At minimum inspect the post-ORC0 versions of:

* `docs/arch/refactor/rev11/authority/**`
* `docs/arch/refactor/rev11/contracts/**`
* `docs/arch/refactor/rev11/tools/programctl.mjs`
* `docs/arch/refactor/rev11/tools/lib.mjs`
* `docs/arch/refactor/rev11/tools/trusted-local.mjs`
* amendment tooling
* lifecycle tests
* evidence schemas
* `.github/workflows/ci.yml`
* `.github/workflows/release-check.yml`
* `.github/workflows/release-tag.yml`
* `.github/workflows/release.yml`
* repository branch/ruleset/merge configuration
* Project 3 fields/items/views
* existing labels/milestones/issues.

Do not assume the current `codex/orc0-trusted-local` implementation is still identical after ORC0 lands. Re-inspect the landed `main` source before authoring the amendment.

# Scoping requirement

Before implementation, run the architect/pre-scope step on each proposed block.

If any block contains multiple independently acceptable outcomes, split it before dispatch.

Do not turn this proposal into another oversized train disguised as one block.

Breaking changes to orchestration tooling are allowed where they materially improve the architecture.

Prefer deletion/replacement over maintaining dual legacy/GitHub orchestration systems.

Do not create a permanent compatibility layer.

# End state

The desired operating experience is approximately:

```
maintainer creates/plans milestones
               │
               ▼
      GitHub Project 3
               │
      train / block issues
               │
           labels
               │
    programctl READY frontier
               │
               ▼
    agent picks admitted block
               │
        draft GitHub PR
               │
   implementation/review/fix
               │
        Ready for review
               │
   formal review + GitHub CI
               │
     immutable gate evidence
               │
   orchestrator squash-merge
               │
      landed-tree proof
               │
     successor becomes READY
```

Separately:

```
non-DAG issue
    ↓
AI inspect
    ↓
.feedback/issues/N.md
    ↓
AI result label only
    ↓
maintainer decides
    ↓
optional authorized DAG promotion
```

And release:

```
milestone
   ↓
READY work completed
   ↓
release planner
   ↓
existing release rehearsal
   ↓
release PR
   ↓
squash `release: vX`
   ↓
existing tag workflow
   ↓
existing release workflow
```

The final system should make GitHub pleasant enough to act as the day-to-day orchestration/history surface while retaining ORC0's stronger local/static evidence guarantees underneath it.

# Binding finding-retention invariant

No finding is lost across acceptance. P0/P1 remain non-dispositionable blockers. Any actionable lower-severity finding that survives acceptance must be either explicitly accepted as non-actionable risk under an owning policy, or materialized as a uniquely fingerprinted carry-forward obligation bound to an immutable receipt and durable GitHub issue. A carried finding remains live until a later immutable resolution receipt supersedes it. Mutable GitHub issue state, labels, milestone movement, or closure cannot erase the obligation. Repeated carry-forward requires escalating authorization and is never implicit.

