<!-- unified-charter-v2
id=GH4
name=PR body, review history and immutable evidence bridge
predecessors=GH3
conditional_predecessors=
phase=governance
train=governance.github-control-plane
product=github_control_plane
kind=implementation
semantic_role=delivery
class=successor
owner=governance.github-control-plane:managed human-readable PR history bound to immutable review evidence
conflict_domains=github_projection_state,feedback_operations
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
charter=charters/governance-github-control-plane/GH4.md
size=M
max_production_loc=800
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# GH4 — PR body, review history and immutable evidence bridge

Authority state is derived at dispatch. PR presentation updates require the current accepted ORC0 activation receipt and current GH3 binding receipt.

## Independently acceptable outcome

Produce one bounded managed PR description and immutable bridge from the current PR revision to Rev11 review evidence. Pre-scope result: human-readable review history and exact evidence binding form one presentation/provenance boundary; CI execution and landing remain GH5.

## Concrete surfaces and APIs

- Production surfaces: `scripts/githubctl`, `docs/arch/refactor/rev11/contracts/github-control-plane.md`.
- Test surfaces: `scripts/githubctl/tests`, `docs/arch/refactor/rev11/fixtures/github`.
- Named boundaries: `ManagedPullRequestBody`, `ReviewCycleSummary`, `ImplementerIdentity`, `ReviewEvidenceBinding`, `FindingCarryForward`.

## Exact predecessor contracts

- **GH3:** exact current receipt ID and digest for “PR-backed dispatch and draft lifecycle”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- Manage concise Problem, Scope, Validation, bounded review-cycle detail sections, and a final Implementer section.
- Preserve human content outside the managed region byte-for-byte; prefer managed-body updates over noisy per-cycle comments.
- Keep raw gate/review logs in immutable Rev11 evidence and bind the managed summary to exact evidence digests and PR revision.
- Implement finding carry-forward records with unique fingerprints, immutable receipt identity, durable GitHub issue database identity/URL, owner, authorization tier, and generation.
- P0/P1 cannot be dispositioned. Lower actionable findings require explicit policy-owned non-actionable acceptance or an OPEN carry-forward. Immutable resolution receipts alone supersede carried findings.
- Mutable GitHub issue state, labels, milestone movement, closure, PR text, or comments cannot erase or resolve a carry-forward obligation.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **GH4-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **GH4-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **GH4-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **GH4-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `docs/arch/refactor/rev11/tools/github-control-plane-authority.test.mjs` and the future deterministic fake-adapter fixtures owned by this node; live GitHub is not a test substrate.

## Deletions and forbidden designs

- Delete or structurally reject whole-body replacement, raw-log dumps, comment-per-cycle spam, PASS reports with findings, mutable issue closure as resolution, and implicit repeated carry-forward.
- Do not execute CI or merge PRs in this block.

## Budgets and mandatory rescope

- Target ceiling: 800 production LOC, 8 production files, 2 related packages.
- Rescope if immutable finding lifecycle cannot remain separate from the mutable GitHub issue record.
- Correctness budget: zero lost human byte, lost finding, forged resolution, stale review presentation, or wrong PR revision.
- Performance budget: when preflight identifies touched authority or a hot path, equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance remains 0.0% unless an owning-authority amendment supplies exact replacement thresholds. Otherwise performance evidence is not applicable; do not create counters or a 100-request retention soak solely to satisfy this charter.

## Abort conditions

- Abort on invalid managed boundaries, missing evidence digest, changed PR head, nonunique finding fingerprint, missing durable issue identity, or insufficient repeat authorization.
- Abort if P0/P1 could reach acceptance through any disposition branch.

## Targeted verification

1. `node --test scripts/githubctl/tests/*.test.mjs`
2. `node docs/arch/refactor/rev11/tools/validate-program-dag.mjs --strict`
3. Run every final command in the bound `canonical` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
4. Bind the preflight evidence selection and terse rationale to the candidate SHA/tree. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and finding retention

Final acceptance requires the complete 3/3 current-round profile with distinct `adversarial`, `conformance`, and `supply-chain-platform` PASS reports. P0/P1 are non-dispositionable. Carry-forward and immutable resolution receipts remain authority regardless of mutable GitHub state.

## Citations

- `source:github-control-plane-program.md:L1`
- `docs/arch/refactor/rev11/contracts/github-control-plane.md`

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-GITHUB-GH4-BLOCK

- Kind: `requirement`
- Source: `github-control-plane-program.md:220-269`
- Applicability: `GH4`
- Exact text SHA-256: `36f90c318494dd0a2cc62c660a991cf55f732bdb097da57310224b8f8c06a815`

~~~~markdown
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
~~~~
