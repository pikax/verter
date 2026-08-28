<!-- unified-charter-v2
id=FB1
name=Non-DAG issue inspection
predecessors=GH1,FB0
conditional_predecessors=
phase=governance
train=governance.feedback-intake
product=feedback_intake
kind=implementation
semantic_role=delivery
class=successor
owner=governance.feedback-intake:evidence-backed issue inspection with AI-owned label-only mutation
conflict_domains=feedback_operations,github_projection_state
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
charter=charters/governance-feedback-intake/FB1.md
size=M
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# FB1 — Non-DAG issue inspection

Authority state is derived at dispatch. Inspection requires the current accepted ORC0 activation receipt plus current GH1 and FB0 receipts.

## Independently acceptable outcome

Implement evidence-backed non-DAG issue inspection with local operational reporting and AI-result-label-only mutation. Pre-scope result: retrieval, current-tree verification, report generation, and one owned label transition are one auditable inspection transaction; DAG promotion remains FB2.

## Concrete surfaces and APIs

- Production surfaces: `scripts/githubctl`, `.feedback`.
- Test surfaces: `scripts/githubctl/tests`, `docs/arch/refactor/rev11/fixtures/github`.
- Named boundaries: `issue inspect <id>`, `IssueInspection`, `FeedbackReport`, `AiIssueVerdict`, exact inspected SHA/tree.

## Exact predecessor contracts

- **GH1:** exact current receipt ID and digest for “GitHub adapter, Project discovery and deterministic fixtures”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **FB0:** exact current receipt ID and digest for “Non-DAG issue label state machine and feedback contract”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- Retrieve issue by stable ID and inspect current source/tests rather than trusting stale issue prose.
- Produce `.feedback/issues/<issue-number>.md` containing issue identity, inspected tree, classification, reproduction, code paths, commands, verdict, confidence/ambiguity, owner hint, and recommendation.
- Update exactly one AI-owned result label; preserve all unrelated, maintainer, and DAG labels.
- Treat `ai:ignore` as a complete no-op with zero report/label mutation.
- Never close/reopen/comment/rewrite/move milestone/promote. `.feedback` is operational evidence, not static DAG authority.
- Inspection may identify a carry-forward-linked issue, but mutable GitHub state cannot erase its immutable obligation or resolve P0/P1.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **FB1-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **FB1-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **FB1-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **FB1-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `docs/arch/refactor/rev11/tools/github-control-plane-authority.test.mjs` and the future deterministic fake-adapter fixtures owned by this node; live GitHub is not a test substrate.

## Deletions and forbidden designs

- Delete or structurally reject issue-body trust without verification, comments/closure/title edits, milestone writes, whole-label replacement, tracked `.feedback` commits by default, and inspection-triggered promotion.
- Do not mutate static DAG authority or create a DAG issue in this block.

## Budgets and mandatory rescope

- Target ceiling: 800 production LOC, 8 production files, 2 related packages.
- Rescope if inspection becomes a general autonomous issue-resolution or repository-mutation engine.
- Correctness budget: zero wrong issue, stale-tree verdict, forbidden field mutation, ignored-issue work, or finding erasure.
- Performance budget: when preflight identifies touched authority or a hot path, equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance remains 0.0% unless an owning-authority amendment supplies exact replacement thresholds. Otherwise performance evidence is not applicable; do not create counters or a 100-request retention soak solely to satisfy this charter.

## Abort conditions

- Abort on stale activation, missing permissions, ambiguous issue identity, `ai:ignore`, unavailable evidence for a confident verdict, or planned mutation outside the AI namespace.
- Use needs-human rather than guessing a product decision.

## Targeted verification

1. `node --test scripts/githubctl/tests/*.test.mjs`
2. `node docs/arch/refactor/rev11/tools/validate-program-dag.mjs --strict`
3. Run every final command in the bound `canonical` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
4. Bind the preflight evidence selection and terse rationale to the candidate SHA/tree. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and finding retention

Final acceptance requires the complete 3/3 current-round profile with distinct `adversarial`, `conformance`, and `supply-chain-platform` PASS reports. P0/P1 block. An inspection verdict or mutable GitHub closure never resolves an immutable carry-forward obligation.

## Citations

- `source:github-control-plane-program.md:L1`
- `docs/arch/refactor/rev11/contracts/github-control-plane.md`

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-GITHUB-FB1-BLOCK

- Kind: `requirement`
- Source: `github-control-plane-program.md:389-449`
- Applicability: `FB1`
- Exact text SHA-256: `77743b832f084b87bcdd061672b8b3fdbcc02fc0f0b6141ba0fbb2415b51ae6f`

~~~~markdown
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
~~~~
