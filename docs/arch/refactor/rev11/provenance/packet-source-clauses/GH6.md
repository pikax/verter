# Exact operative source-clause attachment — GH6

Schema: 1. Node: `GH6`. Clause count: 6. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-GITHUB-END-STATE

- Kind: `acceptance`; source: `github-control-plane-program.md:819-891`; target: `node:GH6`; text SHA-256: `afe1529737e337797a1c38ac371ffa30cce47c1dbf17f91a252b83c1341232fe`.

~~~~markdown
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
~~~~

### SRC-GITHUB-FINDING-RETENTION

- Kind: `requirement`; source: `github-control-plane-program.md:893-895`; target: `node:GH0`; text SHA-256: `7bc0237c2c5f045ecd2510049bad8901064e0de8fefbb6638e2625b9c2c59b98`.

~~~~markdown
# Binding finding-retention invariant

No finding is lost across acceptance. P0/P1 remain non-dispositionable blockers. Any actionable lower-severity finding that survives acceptance must be either explicitly accepted as non-actionable risk under an owning policy, or materialized as a uniquely fingerprinted carry-forward obligation bound to an immutable receipt and durable GitHub issue. A carried finding remains live until a later immutable resolution receipt supersedes it. Mutable GitHub issue state, labels, milestone movement, or closure cannot erase the obligation. Repeated carry-forward requires escalating authorization and is never implicit.
~~~~

### SRC-GITHUB-GH6-BLOCK

- Kind: `requirement`; source: `github-control-plane-program.md:617-651`; target: `node:GH6`; text SHA-256: `2ee0840010453e6938b2440b7622924bc4ad3310cf61df50391376b88a9589c3`.

~~~~markdown
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
~~~~

### SRC-GITHUB-PRESCOPE

- Kind: `requirement`; source: `github-control-plane-program.md:783-817`; target: `node:GH0`; text SHA-256: `31145063dbbd69d54fe57bd2c5f02656b653b4ce0ec7473773e8fac9164369b3`.

~~~~markdown
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
~~~~

### SRC-GITHUB-PROGRAM-GOAL

- Kind: `context`; source: `github-control-plane-program.md:1-45`; target: `node:GH0`; text SHA-256: `171f0f26830374bf948968acfe28e8eab792f78c3fcb1c5ed3de05896b76b534`.

~~~~markdown
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
~~~~

### SRC-GITHUB-REQUIRED-TESTS

- Kind: `acceptance`; source: `github-control-plane-program.md:750-781`; target: `node:GH6`; text SHA-256: `c6d6a3a2e86edcc32d09054ae55bce9f91c10f5ac1acbb905f29ee86a9519e0f`.

~~~~markdown
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
~~~~
