# Exact operative source-clause attachment — GH2

Schema: 1. Node: `GH2`. Clause count: 6. Generated from `provenance/source-coverage.toml`; every clause below is exact, operative, and applicable to this node.

### SRC-GITHUB-FINDING-RETENTION

- Kind: `requirement`; source: `github-control-plane-program.md:893-895`; target: `node:GH0`; text SHA-256: `7bc0237c2c5f045ecd2510049bad8901064e0de8fefbb6638e2625b9c2c59b98`.

~~~~markdown
# Binding finding-retention invariant

No finding is lost across acceptance. P0/P1 remain non-dispositionable blockers. Any actionable lower-severity finding that survives acceptance must be either explicitly accepted as non-actionable risk under an owning policy, or materialized as a uniquely fingerprinted carry-forward obligation bound to an immutable receipt and durable GitHub issue. A carried finding remains live until a later immutable resolution receipt supersedes it. Mutable GitHub issue state, labels, milestone movement, or closure cannot erase the obligation. Repeated carry-forward requires escalating authorization and is never implicit.
~~~~

### SRC-GITHUB-GH2-BLOCK

- Kind: `requirement`; source: `github-control-plane-program.md:97-163`; target: `node:GH2`; text SHA-256: `529d0b1c1456691afdc20b0f9ddc69a6f206010f03f7f6cb452625d2008bd5da`.

~~~~markdown
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
~~~~

### SRC-GITHUB-OWNERSHIP-MATRIX

- Kind: `requirement`; source: `github-control-plane-program.md:653-697`; target: `node:GH0`; text SHA-256: `a8779a3851d606932c0726955ecdba769ab1ab5217c047f37a06ec0032618cba`.

~~~~markdown
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

### SRC-GITHUB-WRITE-DISCIPLINE

- Kind: `requirement`; source: `github-control-plane-program.md:699-717`; target: `node:GH1`; text SHA-256: `8105e3f80af2ce0b5297d035a08623cad93fef16bd9d2acf9491a044b43713ab`.

~~~~markdown
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
~~~~
