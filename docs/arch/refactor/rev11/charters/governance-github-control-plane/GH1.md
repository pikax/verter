<!-- unified-charter-v2
id=GH1
name=GitHub adapter, Project discovery and deterministic fixtures
predecessors=GH0
conditional_predecessors=
phase=governance
train=governance.github-control-plane
product=github_control_plane
kind=implementation
semantic_role=delivery
class=successor
owner=governance.github-control-plane:replaceable structured GitHub adapter and deterministic fake
conflict_domains=github_projection_state
resource_class=ts-heavy
gate_profile=canonical
review_profile=security-3
implementation_effort_min=high
implementation_effort_default=high
review_effort_min=high
review_effort_default=high
verification_effort_min=high
verification_effort_default=high
confirmation_effort_min=high
confirmation_effort_default=high
dispatchable=true
optional=false
release_gating=none
source_refs=source:github-control-plane-program.md:L1
external_requirements=
activation_gate=ORC0
charter=charters/governance-github-control-plane/GH1.md
size=M
max_production_loc=900
max_production_files=10
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# GH1 — GitHub adapter, Project discovery and deterministic fixtures

Authority state is derived at dispatch. The adapter refuses startup unless the current accepted ORC0 activation receipt and GH0 receipt are current.

## Independently acceptable outcome

Deliver one replaceable structured GitHub adapter, permission doctor, dry-run mutation boundary, and deterministic fake. Pre-scope result: the interface and fake must land together so later reconciliation never couples correctness logic to `gh` or live network state; issue projection remains GH2.

## Concrete surfaces and APIs

- Production surfaces: `scripts/githubctl`, `docs/arch/refactor/rev11/contracts/github-control-plane.md`.
- Test surfaces: `scripts/githubctl/tests`, `docs/arch/refactor/rev11/fixtures/github`.
- Named boundaries: `GitHubAdapter`, `GitHubDoctor`, `GitHubMutationPlan`, `FakeGitHubAdapter`, structured repository/project/label records.

## Exact predecessor contracts

- **GH0:** exact current receipt ID and digest for “GitHub control-plane contract and authority matrix”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- Implement `githubctl` above the existing `programctl` correctness kernel, with all GitHub effects behind `GitHubAdapter`.
- Consume supported `gh issue`, `gh pr`, `gh project`, and `gh api` structured JSON only; never regex-scrape terminal or UI prose.
- Doctor validates auth, repository access, Project 3 access and `project` scope, fields, labels, and required mutation capabilities before any write.
- Every write returns an exact structured plan in check mode and applies the same plan in apply mode using stable IDs.
- The deterministic fake models partial failure, retry, permissions, identity ambiguity, issues, PRs, blockers, labels, milestones, project fields, checks, and merge results without live GitHub.
- Preserve immutable finding receipts and carry-forward state as opaque correctness inputs; mutable GitHub issue state is never their authority.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **GH1-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **GH1-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **GH1-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **GH1-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `docs/arch/refactor/rev11/tools/github-control-plane-authority.test.mjs` and the future deterministic fake-adapter fixtures owned by this node; live GitHub is not a test substrate.

## Deletions and forbidden designs

- Delete or structurally reject direct GitHub calls from `programctl`, unstructured stdout parsing, credential persistence, implicit live-test dependencies, and mutation APIs without check mode.
- Do not implement DAG projection, feedback semantics, release planning, or merge authorization in this block.

## Budgets and mandatory rescope

- Target ceiling: 900 production LOC, 10 production files, 2 related packages.
- Rescope on a third transport implementation, direct lifecycle-kernel coupling, or mixed projection semantics.
- Correctness budget: zero ambiguous mutation, token persistence, prose scraping, or fake/live semantic divergence.
- Performance budget: when preflight identifies touched authority or a hot path, equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance remains 0.0% unless an owning-authority amendment supplies exact replacement thresholds. Otherwise performance evidence is not applicable; do not create counters or a 100-request retention soak solely to satisfy this charter.

## Abort conditions

- Abort if current `gh` structured capabilities cannot represent required identity safely or if live discovery would be needed in tests.
- Abort on a missing current ORC0 receipt, GH0 receipt, permission, stable repository/project identity, or deterministic fake behavior.

## Targeted verification

1. `node --test scripts/githubctl/tests/*.test.mjs`
2. `node docs/arch/refactor/rev11/tools/validate-program-dag.mjs --strict`
3. Run every final command in the bound `canonical` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
4. Bind the preflight evidence selection and terse rationale to the candidate SHA/tree. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and finding retention

Final acceptance requires the complete 3/3 current-round profile with distinct `adversarial`, `conformance`, and `supply-chain-platform` PASS reports. P0/P1 block; any lower-severity carry-forward is immutable, issue-bound, and unaffected by mutable GitHub state.

## Citations

- `source:github-control-plane-program.md:L1`
- `docs/arch/refactor/rev11/contracts/github-control-plane.md`

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-GITHUB-GH1-BLOCK

- Kind: `requirement`
- Source: `github-control-plane-program.md:63-95`
- Applicability: `GH1`
- Exact text SHA-256: `f50e990a38c165bae598925866c2097242adfd1ea349ee09cbb0a47635180782`

~~~~markdown
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
~~~~

### SRC-GITHUB-WRITE-DISCIPLINE

- Kind: `requirement`
- Source: `github-control-plane-program.md:699-717`
- Applicability: `GH1`, `GH2`
- Exact text SHA-256: `8105e3f80af2ce0b5297d035a08623cad93fef16bd9d2acf9491a044b43713ab`

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
