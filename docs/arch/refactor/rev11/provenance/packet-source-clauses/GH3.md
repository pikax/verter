# Exact operative source-clause attachment — GH3

Schema: 1. Node: `GH3`. Clause count: 5. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-GITHUB-FINDING-RETENTION

- Kind: `requirement`; source: `github-control-plane-program.md:893-895`; target: `node:GH0`; text SHA-256: `7bc0237c2c5f045ecd2510049bad8901064e0de8fefbb6638e2625b9c2c59b98`.

~~~~markdown
# Binding finding-retention invariant

No finding is lost across acceptance. P0/P1 remain non-dispositionable blockers. Any actionable lower-severity finding that survives acceptance must be either explicitly accepted as non-actionable risk under an owning policy, or materialized as a uniquely fingerprinted carry-forward obligation bound to an immutable receipt and durable GitHub issue. A carried finding remains live until a later immutable resolution receipt supersedes it. Mutable GitHub issue state, labels, milestone movement, or closure cannot erase the obligation. Repeated carry-forward requires escalating authorization and is never implicit.
~~~~

### SRC-GITHUB-GH3-BLOCK

- Kind: `requirement`; source: `github-control-plane-program.md:165-218`; target: `node:GH3`; text SHA-256: `61808ceeb98047a893c22b8b45de2e3396d7861ceb2ecf8d1ebfbdf54042d641`.

~~~~markdown
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
~~~~

### SRC-GITHUB-PR-POLICY

- Kind: `requirement`; source: `github-control-plane-program.md:719-748`; target: `node:GH3`; text SHA-256: `15bd2cb677d2c2ac5ecb4d708ba0e73e5423e610f0d78b48d3e3677e83ec9027`.

~~~~markdown
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
