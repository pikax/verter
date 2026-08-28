<!-- unified-charter-v2
id=FB2
name=Maintainer-authorized issue → DAG promotion
predecessors=FB1,GH2
conditional_predecessors=
phase=governance
train=governance.feedback-intake
product=feedback_intake
kind=implementation
semantic_role=delivery
class=successor
owner=governance.feedback-intake:ratified amendment promotion reusing the durable source issue
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
charter=charters/governance-feedback-intake/FB2.md
size=M
max_production_loc=600
max_production_files=8
max_related_packages=2
rescope_loc=1500
rescope_files=12
rescope_unrelated_packages=3
initial_state=
-->

# FB2 — Maintainer-authorized issue → DAG promotion

Authority state is derived at dispatch. Promotion requires the current accepted ORC0 activation receipt plus current FB1 and GH2 receipts.

## Independently acceptable outcome

Implement one fail-closed bridge from an inspected non-DAG issue through explicit maintainer authorization and the normal post-ORC0 amendment/ratification mechanism to reuse that issue as DAG work. Pre-scope result: authorization proof, amendment provenance, and issue reuse must be atomic; this node does not implement the promoted product work.

## Concrete surfaces and APIs

- Production surfaces: `scripts/githubctl`, `docs/arch/refactor/rev11/contracts/github-control-plane.md`.
- Test surfaces: `scripts/githubctl/tests`, `docs/arch/refactor/rev11/fixtures/github`.
- Named boundaries: `PromotionAuthorization`, `IssuePromotionProposal`, Rev11 amendment receipt, `IssueBinding` reuse, source-issue provenance.

## Exact predecessor contracts

- **FB1:** exact current receipt ID and digest for “Non-DAG issue inspection”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **GH2:** exact current receipt ID and digest for “DAG → GitHub issue projection and reconciliation”; Git ancestry, touched-blob integration equivalence, and receipt currency must validate at dispatch and acceptance.
- **External custody:** no node-specific external authorization beyond the package activation boundary.

## Source-specific scope

- Require explicit maintainer authorization; `dag:promotion-approved` is visible evidence but mutable label state alone is never sufficient.
- Route the architect proposal through the normal amendment mechanism with exact train/node/predecessors/charter and source issue provenance.
- After ratification, reuse the original issue and append only the bounded managed DAG region, labels, parent, and blocker edges.
- Preserve original discussion, body outside managed markers, milestone, and unrelated labels.
- Refuse unauthorized, stale, ambiguous, already-conflicting, or unratified promotion with zero GitHub/authority mutation.
- Promotion or issue closure cannot disposition P0/P1 or erase a lower-severity immutable carry-forward obligation.

## Acceptance IDs and discriminating proof

Preflight evidence selection: preserve all four acceptance outcomes below, then select the smallest evidence set that actually discriminates the touched contract. Existing behavioral coverage, compiler/type/capability enforcement, static validation, canonical gates, bounded inspection, and benchmarks are valid when accompanied by a terse rationale.

- **FB2-AC1 — sole-owner outcome:** the named final owner must be sole and every displaced route named below must be deleted or structurally rejected. Prefer existing type, capability, dependency, compiler, or static enforcement. Add a negative or mutation test only for a plausible critical fail-closed/correctness boundary or a reproduced defect that existing evidence does not discriminate.
- **FB2-AC2 — positive contract:** the named API/data boundary must preserve exact identities, provenance, completeness, and deterministic ordering. Reuse existing coverage or extend/table-drive one test before creating a new test.
- **FB2-AC3 — incremental equivalence:** when the changed scope owns or affects incremental, cache, cancellation, stale-publication, or partial-result authority, prove incremental equals fresh and degraded outcomes cannot warm. Otherwise record a terse not-applicable rationale tied to the untouched authority.
- **FB2-AC4 — bounded work:** when the changed scope owns or affects a hot path, prove no hidden duplicate parse, resolve, plan, emit, copy, allocation, or retained candidate using applicable existing counters, inspection, or benchmarks. Otherwise record a terse not-applicable rationale; do not add counters or a soak by default.
- Every proposed new test must name a plausible regression or contract boundary not already discriminated; prose/format assertions are allowed only when those bytes are the public contract. Do not add implementation mirrors, duplicate permutations, or universal negative/mutation tests.
- Test homes: `docs/arch/refactor/rev11/tools/github-control-plane-authority.test.mjs` and the future deterministic fake-adapter fixtures owned by this node; live GitHub is not a test substrate.

## Deletions and forbidden designs

- Delete or structurally reject label-only DAG mutation, AI-created authorization, duplicate issue creation, overwritten human text, promotion without provenance, and product implementation inside promotion.
- Do not resolve carry-forward findings by promotion or mutable GitHub state.

## Budgets and mandatory rescope

- Target ceiling: 600 production LOC, 8 production files, 2 related packages.
- Rescope if promotion requires bypassing or duplicating amendment tooling.
- Correctness budget: zero unauthorized authority mutation, duplicate issue, lost provenance, human-field loss, or finding erasure.
- Performance budget: when preflight identifies touched authority or a hot path, equivalent-work counters may increase by 0 and wall/allocation/RSS regression allowance remains 0.0% unless an owning-authority amendment supplies exact replacement thresholds. Otherwise performance evidence is not applicable; do not create counters or a 100-request retention soak solely to satisfy this charter.

## Abort conditions

- Abort on stale activation, missing maintainer authorization, missing ratification, changed source issue, marker conflict, or inability to reuse the issue safely.
- Abort rather than synthesize authority from GitHub labels.

## Targeted verification

1. `node --test scripts/githubctl/tests/*.test.mjs`
2. `node docs/arch/refactor/rev11/tools/validate-program-dag.mjs --strict`
3. Run every final command in the bound `canonical` profile on the landing-frozen candidate; targeted success alone is iteration evidence, not acceptance.
4. Bind the preflight evidence selection and terse rationale to the candidate SHA/tree. Behavioral code changes require TDD with a failing discriminating regression before production changes; do not invent a test or mutation solely to populate evidence.

## Review and finding retention

Final acceptance requires the complete 3/3 current-round profile with distinct `adversarial`, `conformance`, and `supply-chain-platform` PASS reports. P0/P1 block. Promotion preserves any immutable carry-forward obligation regardless of mutable GitHub issue state and repeated carry-forward still requires escalating authorization.

## Citations

- `source:github-control-plane-program.md:L1`
- `docs/arch/refactor/rev11/contracts/github-control-plane.md`

## Transferred source requirement atoms

These clauses are operative only for the exact applicability set shown. Cold packets include the exact applicable subset and its source digest.

### SRC-GITHUB-FB2-BLOCK

- Kind: `requirement`
- Source: `github-control-plane-program.md:451-486`
- Applicability: `FB2`
- Exact text SHA-256: `5dbe9e2ab2a352739ceb903be4d41a362603503026d560f6658fba65c9c82ab3`

~~~~markdown
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
~~~~
